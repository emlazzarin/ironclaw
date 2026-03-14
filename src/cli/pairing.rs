//! DM pairing CLI commands.
//!
//! Manage pairing requests for channels (Telegram, Slack, etc.).

use clap::Subcommand;

use crate::pairing::PairingStore;

/// Pairing subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum PairingCommand {
    /// List pending pairing requests
    List {
        /// Optional channel name filter (e.g., telegram, slack)
        channel: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Approve a pairing request by code
    Approve {
        /// Channel name (e.g., telegram, slack)
        #[arg(required = true)]
        channel: String,

        /// Pairing code (e.g., ABC12345)
        #[arg(required = true)]
        code: String,
    },
}

/// Run pairing CLI command.
pub fn run_pairing_command(cmd: PairingCommand) -> Result<(), String> {
    run_pairing_command_with_store(&PairingStore::new(), cmd)
}

/// Run pairing CLI command with a given store (for testing).
pub fn run_pairing_command_with_store(
    store: &PairingStore,
    cmd: PairingCommand,
) -> Result<(), String> {
    match cmd {
        PairingCommand::List { channel, json } => run_list(store, channel.as_deref(), json),
        PairingCommand::Approve { channel, code } => run_approve(store, &channel, &code),
    }
}

fn run_list(store: &PairingStore, channel: Option<&str>, json: bool) -> Result<(), String> {
    match channel {
        Some(channel) => run_list_for_channel(store, channel, json),
        None => run_list_all(store, json),
    }
}

fn run_list_for_channel(store: &PairingStore, channel: &str, json: bool) -> Result<(), String> {
    let requests = store.list_pending(channel).map_err(|e| e.to_string())?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&requests).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    if requests.is_empty() {
        println!("No pending {} pairing requests.", channel);
        return Ok(());
    }

    println!("Pairing requests ({}):", requests.len());
    for r in &requests {
        print_request(r);
    }

    Ok(())
}

fn run_list_all(store: &PairingStore, json: bool) -> Result<(), String> {
    let channels = store.list_pending_all().map_err(|e| e.to_string())?;

    if json {
        let grouped = channels
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&grouped).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    if channels.is_empty() {
        println!("No pending pairing requests.");
        return Ok(());
    }

    let total_requests: usize = channels.iter().map(|(_, requests)| requests.len()).sum();
    println!(
        "Pending pairing requests ({} across {} channels):",
        total_requests,
        channels.len()
    );

    for (channel, requests) in &channels {
        println!();
        println!("{} ({})", channel, requests.len());
        for r in requests {
            print_request(r);
        }
    }

    Ok(())
}

fn print_request(request: &crate::pairing::PairingRequest) {
    println!(
        "  {}  {}  {}  {}",
        request.code,
        request.id,
        request_meta(request),
        request.created_at
    );
}

fn request_meta(request: &crate::pairing::PairingRequest) -> String {
    request
        .meta
        .as_ref()
        .and_then(|meta| meta.as_object())
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|value| format!("{key}={value}")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn run_approve(store: &PairingStore, channel: &str, code: &str) -> Result<(), String> {
    match store.approve(channel, code) {
        Ok(Some(entry)) => {
            println!("Approved {} sender {}.", channel, entry.id);
            Ok(())
        }
        Ok(None) => Err(format!(
            "No pending pairing request found for code: {}",
            code
        )),
        Err(crate::pairing::PairingStoreError::ApproveRateLimited) => Err(
            "Too many failed approve attempts. Wait a few minutes before trying again.".to_string(),
        ),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_store() -> (PairingStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = PairingStore::with_base_dir(dir.path().to_path_buf());
        (store, dir)
    }

    #[test]
    fn test_list_empty_returns_ok() {
        let (store, _) = test_store();
        let result = run_pairing_command_with_store(
            &store,
            PairingCommand::List {
                channel: Some("telegram".to_string()),
                json: false,
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_json_empty_returns_ok() {
        let (store, _) = test_store();
        let result = run_pairing_command_with_store(
            &store,
            PairingCommand::List {
                channel: Some("telegram".to_string()),
                json: true,
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_approve_invalid_code_returns_err() {
        let (store, _) = test_store();
        // Create a pending request so the pairing file exists, then approve with wrong code
        store.upsert_request("telegram", "user1", None).unwrap();

        let result = run_pairing_command_with_store(
            &store,
            PairingCommand::Approve {
                channel: "telegram".to_string(),
                code: "BADCODE1".to_string(),
            },
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No pending pairing request"));
    }

    #[test]
    fn test_approve_valid_code_returns_ok() {
        let (store, _) = test_store();
        let r = store.upsert_request("telegram", "user1", None).unwrap();
        assert!(r.created);

        let result = run_pairing_command_with_store(
            &store,
            PairingCommand::Approve {
                channel: "telegram".to_string(),
                code: r.code,
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_with_pending_returns_ok() {
        let (store, _) = test_store();
        store.upsert_request("telegram", "user1", None).unwrap();

        let result = run_pairing_command_with_store(
            &store,
            PairingCommand::List {
                channel: Some("telegram".to_string()),
                json: false,
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_all_with_pending_returns_ok() {
        let (store, _) = test_store();
        store.upsert_request("telegram", "user1", None).unwrap();
        store.upsert_request("slack", "user2", None).unwrap();

        let result = run_pairing_command_with_store(
            &store,
            PairingCommand::List {
                channel: None,
                json: false,
            },
        );
        assert!(result.is_ok());
    }
}
