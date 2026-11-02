use super::*;
use crate::{config::SecurityConfig, db::Database};
use std::sync::Arc;

const SECRET: &str = "test-secret-at-least-thirty-two-bytes";

pub async fn auth_fixture() -> (AuthContext, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::connect(&format!(
            "sqlite://{}",
            directory.path().join("kis.db").display()
        ))
        .await
        .unwrap(),
    );
    let password = AuthContext::hash_password("correct-password").unwrap();
    sqlx::query("INSERT INTO users (id, username, password_hash, display_name, email, role, created_at, updated_at)
        VALUES ('u1', 'alice', ?, 'Alice', 'alice@example.test', 'doctor', datetime('now'), datetime('now'))")
        .bind(password).execute(db.pool()).await.unwrap();
    let audit = Arc::new(
        AuditLogger::start(&directory.path().join("audit.jsonl"), db.pool().clone())
            .await
            .unwrap(),
    );
    let context =
        AuthContext::from_secret(&SecurityConfig::default(), db, audit, SECRET.into()).unwrap();
    (context, directory)
}

async fn serve(context: AuthContext) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stack = Arc::new(SecurityStack {
        audit: context.audit.clone(),
        auth: Arc::new(context),
        limiter: Arc::new(SlidingWindowLimiter::default()),
    });
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            crate::api::create_router(stack)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}"), server)
}

#[tokio::test]
async fn documented_routes_login_and_unknown_user() {
    let (context, _directory) = auth_fixture().await;
    let (base, server) = serve(context).await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{base}/api/health"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "OK"
    );
    assert_eq!(
        client
            .get(format!("{base}/api/api/health"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    for (username, expected) in [("unknown", 401), ("alice", 200)] {
        let response = client
            .post(format!("{base}/api/auth/login"))
            .json(&serde_json::json!({"username": username, "password": "correct-password"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    server.abort();
}

#[tokio::test]
async fn refresh_is_not_access_and_revocation_survives_context_restart() {
    let (context, _directory) = auth_fixture().await;
    let (access, refresh) = context.issue_tokens("u1", "alice", "doctor").await.unwrap();
    assert!(context.verify_access_token(&access).await.is_ok());
    assert!(context.verify_access_token(&refresh).await.is_err());
    context.revoke_refresh(&refresh).await.unwrap();
    let restarted = AuthContext::from_secret(
        &SecurityConfig::default(),
        context.db.clone(),
        context.audit.clone(),
        SECRET.into(),
    )
    .unwrap();
    assert!(restarted.verify_access_token(&access).await.is_err());
    assert!(restarted.refresh_tokens(&refresh).await.is_err());
}

#[tokio::test]
async fn refresh_rotation_has_only_one_winner() {
    let (context, _directory) = auth_fixture().await;
    let (access, refresh) = context.issue_tokens("u1", "alice", "doctor").await.unwrap();
    let (first, second) = tokio::join!(
        context.refresh_tokens(&refresh),
        context.refresh_tokens(&refresh)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(context.verify_access_token(&access).await.is_err());
    let pair = first.or(second).unwrap();
    assert!(context.verify_access_token(&pair.0).await.is_ok());
}

#[tokio::test]
async fn route_roles_cannot_be_overridden_by_query() {
    let (context, _directory) = auth_fixture().await;
    let (access, refresh) = context.issue_tokens("u1", "alice", "doctor").await.unwrap();
    let (base, server) = serve(context).await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(format!("{base}/api/patients?role=reception"))
            .bearer_auth(&access)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{base}/api/patients"))
            .bearer_auth(&refresh)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/api/patients"))
            .bearer_auth(&access)
            .send()
            .await
            .unwrap()
            .status(),
        501
    );
    assert_eq!(
        client
            .post(format!("{base}/api/auth/mfa/setup"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{base}/api/auth/mfa/setup"))
            .bearer_auth(&access)
            .send()
            .await
            .unwrap()
            .status(),
        501
    );
    assert_eq!(
        client
            .post(format!("{base}/api/auth/logout"))
            .bearer_auth(&access)
            .json(&serde_json::json!({"refresh_token": refresh}))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        client
            .get(format!("{base}/api/patients"))
            .bearer_auth(&access)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    server.abort();
}

#[tokio::test]
async fn inactive_user_cannot_use_existing_tokens() {
    let (context, _directory) = auth_fixture().await;
    let (access, refresh) = context.issue_tokens("u1", "alice", "doctor").await.unwrap();
    sqlx::query("UPDATE users SET is_active = 0 WHERE id = 'u1'")
        .execute(context.db.pool())
        .await
        .unwrap();
    assert!(context.verify_access_token(&access).await.is_err());
    assert!(context.refresh_tokens(&refresh).await.is_err());
}

#[tokio::test]
async fn audit_concurrent_entries_and_content_tampering() {
    let (context, _directory) = auth_fixture().await;
    let mut tasks = Vec::new();
    for id in 0..100 {
        let audit = context.audit.clone();
        tasks.push(tokio::spawn(async move {
            audit
                .log(
                    Some("u1"),
                    AuditAction::Read,
                    "patient",
                    Some(&id.to_string()),
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
        .fetch_one(context.db.pool())
        .await
        .unwrap();
    assert_eq!(count, 100);
    AuditLogger::verify_chain(context.db.pool()).await.unwrap();
    sqlx::query("UPDATE audit_log SET entity_type = 'tampered' WHERE id = 50")
        .execute(context.db.pool())
        .await
        .unwrap();
    assert!(AuditLogger::verify_chain(context.db.pool()).await.is_err());
    assert!(AuditLogger::start(
        &_directory.path().join("restart.jsonl"),
        context.db.pool().clone()
    )
    .await
    .is_err());
}

#[tokio::test]
async fn audit_failure_blocks_data_handler() {
    let (context, _directory) = auth_fixture().await;
    let (access, _) = context.issue_tokens("u1", "alice", "doctor").await.unwrap();
    sqlx::query("DROP TABLE audit_log")
        .execute(context.db.pool())
        .await
        .unwrap();
    let (base, server) = serve(context).await;
    let response = reqwest::Client::new()
        .get(format!("{base}/api/patients"))
        .bearer_auth(access)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    server.abort();
}

#[tokio::test]
async fn audit_and_session_writes_can_run_together() {
    let (context, _directory) = auth_fixture().await;
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let context = context.clone();
        tasks.push(tokio::spawn(async move {
            context.issue_tokens("u1", "alice", "doctor").await.unwrap();
            context
                .audit
                .log(
                    Some("u1"),
                    AuditAction::Login,
                    "user",
                    Some("u1"),
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    AuditLogger::verify_chain(context.db.pool()).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
        .fetch_one(context.db.pool())
        .await
        .unwrap();
    assert_eq!(count, 20);
}

#[tokio::test]
async fn login_attempt_column_enforces_lockout() {
    let (context, _directory) = auth_fixture().await;
    let (base, server) = serve(context).await;
    let client = reqwest::Client::new();
    for _ in 0..5 {
        assert_eq!(
            client
                .post(format!("{base}/api/auth/login"))
                .json(&serde_json::json!({"username":"alice", "password":"wrong"}))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        client
            .post(format!("{base}/api/auth/login"))
            .json(&serde_json::json!({"username":"alice", "password":"correct-password"}))
            .send()
            .await
            .unwrap()
            .status(),
        423
    );
    server.abort();
}
