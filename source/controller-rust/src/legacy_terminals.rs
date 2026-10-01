//! One-way cleanup of panel-owned tmux sessions left by version 0.8.0.
use crate::{core::*, file_sessions, ssh};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};
#[derive(Clone, Deserialize, Serialize)]
struct Legacy {
    id: String,
    node: String,
}
pub fn start(app: &App) {
    let app = app.clone();
    tokio::spawn(async move {
        let path = app.0.dir.join("terminal-sessions.json");
        let Ok(mut pending) = read_json::<HashMap<String, Legacy>>(&path) else {
            return;
        };
        if pending.len() > 32
            || pending
                .iter()
                .any(|(id, r)| id != &r.id || !file_sessions::valid(id))
        {
            return;
        }
        let prefix = format!(
            "tmux -L yuji-{} -f /dev/null",
            &hex::encode(Sha256::digest(app.0.master.as_slice()))[..20]
        );
        let mut timer = tokio::time::interval(Duration::from_secs(15));
        while !pending.is_empty() {
            tokio::select! { _ = app.0.stop.cancelled() => return, _ = timer.tick() => {} }
            for record in pending.values().cloned().collect::<Vec<_>>() {
                let info = {
                    let i = app.lock();
                    i.data
                        .nodes
                        .iter()
                        .find(|n| n.public.id == record.node)
                        .cloned()
                        .map(|n| {
                            (
                                n,
                                i.data.secrets.get(&record.node).cloned(),
                                i.agents.get(&record.node).cloned(),
                            )
                        })
                };
                let cleaned = match info {
                    None => true, // Removed nodes must never be contacted again.
                    Some((node, Some(secret), Some(link))) => {
                        let work = async {
                            let client = ssh::agent_client(
                                &app,
                                link,
                                &node.public.id,
                                &node.username,
                                &secret,
                            )
                            .await?;
                            let name = ssh::quote(&format!("yuji_{}", record.id));
                            let command = format!(
                                "{prefix} kill-session -t {name} 2>/dev/null || ! {prefix} has-session -t {name} 2>/dev/null"
                            );
                            let result = ssh::exec(&client, &command, &[], 4096).await;
                            let files_clean =
                                crate::files::transfer::remove_session(&app, &record.id, &client)
                                    .await;
                            let _ = client
                                .disconnect(
                                    russh::Disconnect::ByApplication,
                                    "legacy terminal removed",
                                    "",
                                )
                                .await;
                            result.map(|_| files_clean)
                        };
                        matches!(
                            tokio::time::timeout(Duration::from_secs(15), work).await,
                            Ok(Ok(true))
                        )
                    }
                    _ => false,
                };
                if cleaned {
                    pending.remove(&record.id);
                    let _ = atomic_json(&path, &pending);
                }
            }
        }
        let _ = std::fs::remove_file(path);
    });
}
