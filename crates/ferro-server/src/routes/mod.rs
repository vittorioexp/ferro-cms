mod auth;
mod assistant;
mod collections;
mod files;
mod flows;
mod graphql;
mod items;
mod schema;
mod server;
mod sync;
mod users;
mod workspace;
mod ws;

use axum::routing::{any, delete, get, patch, post, put};
use axum::Router;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        // server
        .route("/server/health", get(server::health))
        .route("/server/info", get(server::info))
        .route("/server/openapi.json", get(crate::openapi::openapi_json))
        // auth
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/methods", get(auth::methods))
        .route("/auth/oauth/exchange", post(auth::oauth_exchange))
        .route("/auth/oauth/:name", get(auth::oauth_start))
        .route("/auth/oauth/:name/callback", get(auth::oauth_callback))
        .route("/auth/password/request", post(auth::password_request))
        .route("/auth/password/reset", post(auth::password_reset))
        .route("/auth/invite/accept", post(auth::invite_accept))
        .route("/auth/tfa/generate", post(auth::tfa_generate))
        .route("/auth/tfa/enable", post(auth::tfa_enable))
        .route("/auth/tfa/disable", post(auth::tfa_disable))
        // items
        .route(
            "/items/:collection",
            get(items::list).post(items::create).patch(items::update_many).delete(items::delete_many),
        )
        .route("/items/:collection/aggregate", post(items::aggregate_items))
        .route("/items/:collection/:id", get(items::get_one).patch(items::update).delete(items::delete_one))
        .route("/items/:collection/:id/publish", post(items::publish_item))
        .route("/items/:collection/:id/draft", delete(items::discard_draft))
        .route("/items/:collection/:id/versions", get(items::list_content_versions))
        .route("/items/:collection/:id/versions/:version_id/restore", post(items::restore_content_version))
        // data model
        .route("/collections", get(collections::list).post(collections::create))
        .route(
            "/collections/:name",
            get(collections::get_one).patch(collections::update).delete(collections::remove),
        )
        .route("/collections/:name/translations", post(collections::add_translations))
        .route("/fields/:collection", post(collections::add_field))
        .route("/fields/:collection/:field", patch(collections::update_field).delete(collections::remove_field))
        .route("/roles", get(collections::list_roles))
        .route("/roles/:id", put(collections::put_role).delete(collections::delete_role))
        .route("/policies", get(collections::list_policies))
        .route("/policies/:id", put(collections::put_policy).delete(collections::delete_policy))
        // schema as code
        .route("/schema/snapshot", get(schema::snapshot))
        .route("/schema/plan", post(schema::plan))
        .route("/schema/apply", post(schema::apply))
        .route("/schema/history", get(schema::history))
        // users
        .route("/users", get(users::list).post(users::create))
        .route("/users/invite", post(users::invite))
        // `/users/me` is handled by get_one
        .route("/users/:id", get(users::get_one).patch(users::update).delete(users::remove))
        .route("/users/:id/token", post(users::rotate_token))
        .route("/users/:id/tfa", delete(users::reset_tfa))
        // files
        .route("/files", get(files::list).post(files::upload))
        .route("/files/:id", get(files::get_one).patch(files::update).delete(files::remove))
        .route("/assets/:id", get(files::asset))
        .route("/folders", get(files::list_folders).post(files::create_folder))
        // flows (automations), custom endpoints, extensions
        .route("/flows", get(flows::list))
        .route("/flows/:id", put(flows::put).delete(flows::remove))
        .route("/flows/:id/run", post(flows::run_manual))
        .route("/flows/:id/runs", get(flows::runs))
        .route("/flow-actions", get(flows::actions))
        .route("/hooks/:id", any(flows::hook_by_id))
        .route("/x/:slug", any(flows::hook_by_slug))
        .route("/server/extensions", get(server::extensions))
        // realtime
        .route("/websocket", get(ws::upgrade))
        // graphql
        .route("/graphql", get(graphql::get).post(graphql::post))
        .route("/graphql/schema", get(graphql::schema_sdl))
        // AI assistant
        .route("/assistant/config", get(assistant::config))
        .route("/assistant/chat", post(assistant::chat))
        // audit
        .route("/activity", get(items::activity))
        .route("/revisions", get(items::revisions))
        // workspace (comments, notifications, presets, dashboard, insights)
        .route("/comments/:id", patch(workspace::patch_comment).delete(workspace::delete_comment))
        .route(
            "/items/:collection/:id/comments",
            get(workspace::list_item_comments).post(workspace::create_item_comment),
        )
        .route("/notifications", get(workspace::list_notifications))
        .route("/notifications/read-all", post(workspace::mark_notifications_read_all))
        .route("/notifications/:id/read", post(workspace::mark_notification_read))
        .route("/presets", get(workspace::list_presets).post(workspace::create_preset))
        .route("/presets/:id", patch(workspace::update_preset).delete(workspace::delete_preset))
        .route("/dashboard", get(workspace::get_dashboard).put(workspace::put_dashboard))
        .route("/insights", get(workspace::insights))
        .route("/insights/recent-activity", get(workspace::recent_activity))
        // environment sync
        .route("/sync/export", post(sync::export))
        .route("/sync/missing-files", post(sync::missing_files))
        .route("/sync/files/:id", put(sync::put_file).get(sync::get_file))
        .route("/sync/import", post(sync::import))
        .route("/sync/state", delete(sync::reset_state))
}
