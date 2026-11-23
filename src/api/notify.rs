//! Gitee's notification inbox (`/notifications/*`).
//!
//! The thread objects are under-documented and vary between deployments, so
//! fields are picked out of the raw JSON by name instead of deserialized
//! strictly: one unexpected shape must not empty the inbox.

use super::*;
use serde_json::Value;

/// The inbox, newest first. `unread_only` asks Gitee to filter.
pub fn list_notifications(auth: AuthMode, unread_only: bool, page: Page) -> Result<Vec<Notification>, ApiError> {
    let url = format!(
        "/notifications/threads?unread={}&{}",
        if unread_only { "true" } else { "false" },
        page.query()
    );
    let items: Vec<Value> = get(&url, auth)?;
    Ok(items.iter().filter_map(parse_thread).collect())
}

/// The unread and total counts (`GET /notifications/count`).
pub fn notification_count(auth: AuthMode) -> Result<NotificationCount, ApiError> {
    let v: Value = get("/notifications/count", auth)?;
    let number = |key: &str| v.get(key).and_then(|n| n.as_u64()).unwrap_or(0) as u32;
    Ok(NotificationCount { unread: number("unread"), total: number("total") })
}

/// Mark one thread read.
pub fn mark_read(auth: AuthMode, id: &str) -> Result<(), ApiError> {
    send_empty("PATCH", &format!("/notifications/threads/{}", enc(id)), Form::new(), auth)
}

/// Mark every thread read.
pub fn mark_all_read(auth: AuthMode) -> Result<(), ApiError> {
    send_empty("PUT", "/notifications/threads", Form::new(), auth)
}

/// One thread out of Gitee's raw JSON.
fn parse_thread(v: &Value) -> Option<Notification> {
    let id = match v.get("id")? {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let subject = v.get("subject");
    let title = subject
        .and_then(|s| s.get("title"))
        .and_then(|t| t.as_str())
        .or_else(|| v.get("title").and_then(|t| t.as_str()))
        .unwrap_or_default()
        .to_owned();
    let kind = subject
        .and_then(|s| s.get("type"))
        .and_then(|t| t.as_str())
        .or_else(|| v.get("type").and_then(|t| t.as_str()))
        .unwrap_or_default()
        .to_owned();
    let url = subject
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .or_else(|| v.get("url").and_then(|u| u.as_str()))
        .map(str::to_owned);
    let repo = ["repository", "repo"]
        .iter()
        .filter_map(|key| v.get(*key))
        .find_map(|r| {
            r.get("full_name")
                .and_then(|n| n.as_str())
                .or_else(|| r.get("human_name").and_then(|n| n.as_str()))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let unread = v.get("unread").and_then(|u| u.as_bool()).unwrap_or(true);
    let updated_at = v.get("updated_at").and_then(|u| u.as_str()).map(str::to_owned);
    Some(Notification { id, title, kind, repo, url, unread, updated_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::json;
    
    #[test]
    fn parses_threads_leniently() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert!(req.url.contains("/notifications/threads?unread=true"), "{}", req.url);
            json(
                200,
                r#"[
                    {"id":11,"unread":true,"updated_at":"2025-01-01","subject":{"title":"New PR","type":"PullRequest","url":"https://gitee.com/acme/widgets/pulls/1"},"repository":{"full_name":"acme/widgets"}},
                    {"id":"12","subject":{"title":"Mention"}},
                    {"no_id":true}
                ]"#,
            )
        });
        let threads = list_notifications(AuthMode::Token, true, Page::first(50)).unwrap();
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "11");
        assert_eq!(threads[0].title, "New PR");
        assert_eq!(threads[0].repo, "acme/widgets");
        assert_eq!(threads[1].id, "12");
        assert!(threads[1].unread, "missing unread counts as unread");
    }

    #[test]
    fn reads_the_counts() {
        let host = crate::api::test_host();
        host.on_fetch(|_| json(200, r#"{"total":12,"unread":3}"#));
        let count = notification_count(AuthMode::Token).unwrap();
        assert_eq!(count.unread, 3);
        assert_eq!(count.total, 12);
    }

    #[test]
    fn marks_one_and_all_read() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert!(req.url.contains("/notifications/threads"));
            json(200, "{}")
        });
        mark_read(AuthMode::Token, "11").unwrap();
        mark_all_read(AuthMode::Token).unwrap();
        let requests = host.requests();
        assert_eq!(requests[0].method, "PATCH");
        assert!(requests[0].url.ends_with("/threads/11"));
        assert_eq!(requests[1].method, "PUT");
    }
}
