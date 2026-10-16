//! Minimal, read-only Oorush PoA validator/node dashboard.
//!
//! The binary is intentionally named `oorush-miner` for compatibility with
//! existing launchers, but it is not a proof-of-work miner. It never hashes
//! candidate blocks, submits transactions, or handles wallet passwords.

use eframe::egui;
use serde::Deserialize;
use std::{
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

const DEFAULT_STATUS_URL: &str = "http://127.0.0.1:18445";
const DEFAULT_NODE_ADDRESS: &str = "127.0.0.1:18444";
const DEFAULT_WALLET_PATH: &str = "oorush-wallet.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeStatus {
    #[serde(default)]
    service: String,
    #[serde(default)]
    chain: String,
    #[serde(default)]
    height: u64,
    #[serde(default)]
    pending: u64,
    #[serde(default)]
    validators: u64,
    #[serde(default)]
    utxos: u64,
    #[serde(default)]
    balance: u64,
    #[serde(default)]
    wallet_address: String,
    #[serde(default)]
    wallet_public_key: String,
    #[serde(default)]
    wallet_state: String,
    #[serde(default)]
    latest_hash: String,
    #[serde(default)]
    transport: String,
    #[serde(default)]
    read_only: bool,
}

struct MinerApp {
    status_url: String,
    node_address: String,
    wallet_path: String,
    status: Option<NodeStatus>,
    message: String,
    refresh_receiver: Option<Receiver<Result<NodeStatus, String>>>,
}

impl Default for MinerApp {
    fn default() -> Self {
        Self {
            status_url: DEFAULT_STATUS_URL.to_owned(),
            node_address: DEFAULT_NODE_ADDRESS.to_owned(),
            wallet_path: DEFAULT_WALLET_PATH.to_owned(),
            status: None,
            message: "No status loaded. Start the node with --http-listen, then refresh."
                .to_owned(),
            refresh_receiver: None,
        }
    }
}

impl MinerApp {
    fn refresh(&mut self) {
        if self.refresh_receiver.is_some() {
            return;
        }

        let url = self.status_url.trim().to_owned();
        let (sender, receiver) = mpsc::channel();
        self.refresh_receiver = Some(receiver);
        self.message = "Refreshing read-only node status…".to_owned();
        std::thread::spawn(move || {
            let result = fetch_status(&url);
            let _ = sender.send(result);
        });
    }

    fn poll_refresh(&mut self) {
        let Some(receiver) = &self.refresh_receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(status)) => {
                self.status = Some(status);
                self.message = "Connected to the node read-only status API.".to_owned();
                self.refresh_receiver = None;
            }
            Ok(Err(error)) => {
                self.message = error;
                self.refresh_receiver = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.message = "Status request stopped before returning a response.".to_owned();
                self.refresh_receiver = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

impl eframe::App for MinerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_refresh();
        ctx.request_repaint_after(Duration::from_millis(100));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Oorush PoA Validator Dashboard");
            ui.label("Research / educational software");
            ui.colored_label(
                egui::Color32::from_rgb(180, 80, 20),
                "Proof of Authority: no CPU mining, hash grinding, or proof-of-work occurs here.",
            );
            ui.add_space(8.0);

            egui::Grid::new("node_configuration")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Status URL");
                    ui.text_edit_singleline(&mut self.status_url);
                    ui.end_row();
                    ui.label("Node TCP address");
                    ui.text_edit_singleline(&mut self.node_address);
                    ui.end_row();
                    ui.label("Wallet path");
                    ui.add_enabled(false, egui::TextEdit::singleline(&mut self.wallet_path));
                    ui.end_row();
                });

            ui.add_space(8.0);
            if ui
                .add_enabled(
                    self.refresh_receiver.is_none(),
                    egui::Button::new("Refresh status"),
                )
                .clicked()
            {
                self.refresh();
            }
            ui.label(&self.message);
            ui.small("Only loopback HTTP status URLs are accepted; the request is GET /status.");

            ui.separator();
            ui.heading("Live node status");
            if let Some(status) = &self.status {
                egui::Grid::new("node_status")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        status_row(ui, "Service", &status.service);
                        status_row(ui, "Chain", &status.chain);
                        status_row(ui, "Height", &status.height.to_string());
                        status_row(ui, "Pending transactions", &status.pending.to_string());
                        status_row(ui, "Validators", &status.validators.to_string());
                        status_row(ui, "UTXOs", &status.utxos.to_string());
                        status_row(ui, "Validator balance", &status.balance.to_string());
                        status_row(ui, "Wallet state", &status.wallet_state);
                        status_row(ui, "Wallet address", &status.wallet_address);
                        status_row(ui, "Wallet public key", &status.wallet_public_key);
                        status_row(ui, "Latest block hash", &status.latest_hash);
                        status_row(ui, "Transport", &status.transport);
                        status_row(ui, "Read-only API", if status.read_only { "yes" } else { "no" });
                    });
            } else {
                ui.label("No live status yet.");
            }

            ui.add_space(8.0);
            ui.small(
                "The node is started separately with oorush-node. This dashboard does not accept, store, or print passwords or private keys.",
            );
        });
    }
}

fn status_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.strong(label);
    ui.label(value);
    ui.end_row();
}

fn fetch_status(url: &str) -> Result<NodeStatus, String> {
    let (address, path, host_header) = parse_status_url(url)?;
    let mut stream = TcpStream::connect_timeout(&address, REQUEST_TIMEOUT)
        .map_err(|error| format!("Could not connect to the local node status API: {error}"))?;
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| format!("Could not configure status read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| format!("Could not configure status write timeout: {error}"))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| format!("Could not request node status: {error}"))?;

    let mut response = Vec::new();
    stream
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|error| format!("Could not read node status: {error}"))?;
    if response.len() > MAX_RESPONSE_BYTES {
        return Err("Node status response is too large.".to_owned());
    }
    let response = String::from_utf8(response)
        .map_err(|_| "Node status response was not valid UTF-8.".to_owned())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "Node status response was malformed.".to_owned())?;
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| "Node status response had no status line.".to_owned())?;
    if !status_line.starts_with("HTTP/1.1 200 ") && !status_line.starts_with("HTTP/1.0 200 ") {
        return Err(format!("Node status API returned {status_line}."));
    }
    let status: NodeStatus = serde_json::from_str(body)
        .map_err(|error| format!("Node status JSON was invalid: {error}"))?;
    if !status.read_only {
        return Err("The node status response did not identify itself as read-only.".to_owned());
    }
    Ok(status)
}

fn parse_status_url(input: &str) -> Result<(SocketAddr, String, String), String> {
    let input = input.trim();
    let authority_and_path = input
        .strip_prefix("http://")
        .ok_or_else(|| "Status URL must use http:// and point to loopback.".to_owned())?;
    if authority_and_path.contains('@') {
        return Err("Status URL must not contain credentials.".to_owned());
    }
    let (authority, path) = authority_and_path
        .split_once('/')
        .map_or((authority_and_path, "/status"), |(authority, _path)| {
            (authority, &authority_and_path[authority.len()..])
        });
    if authority.is_empty() || path.contains('?') || path.contains('#') {
        return Err("Status URL must contain a loopback host and a simple path.".to_owned());
    }

    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest
            .split_once(']')
            .ok_or_else(|| "Status URL has an invalid IPv6 host.".to_owned())?;
        let port = port
            .strip_prefix(':')
            .ok_or_else(|| "Status URL must include a port after an IPv6 host.".to_owned())?
            .parse::<u16>()
            .map_err(|_| "Status URL has an invalid port.".to_owned())?;
        (host, port)
    } else {
        let (host, port) = authority
            .rsplit_once(':')
            .ok_or_else(|| "Status URL must include a port.".to_owned())?;
        let port = port
            .parse::<u16>()
            .map_err(|_| "Status URL has an invalid port.".to_owned())?;
        (host, port)
    };
    let normalized_host = host.trim_matches(['[', ']']);
    let ip = match normalized_host {
        "localhost" | "127.0.0.1" => IpAddr::from([127, 0, 0, 1]),
        "::1" => IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]),
        _ => return Err("Status URL must point to localhost, 127.0.0.1, or ::1.".to_owned()),
    };
    let request_path = if path == "/" { "/status" } else { path };
    let host_header = if normalized_host == "::1" {
        format!("[{normalized_host}]:{port}")
    } else {
        format!("{normalized_host}:{port}")
    };
    Ok((
        SocketAddr::new(ip, port),
        request_path.to_owned(),
        host_header,
    ))
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("oorush-miner — PoA Validator Dashboard")
            .with_inner_size([720.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "oorush-miner — PoA Validator Dashboard",
        options,
        Box::new(|_creation_context| Ok(Box::new(MinerApp::default()))),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_status_url;

    #[test]
    fn status_url_defaults_root_to_status_endpoint() {
        let (_, path, host) = parse_status_url("http://127.0.0.1:18445").unwrap();
        assert_eq!(path, "/status");
        assert_eq!(host, "127.0.0.1:18445");
    }

    #[test]
    fn status_url_rejects_non_loopback_hosts() {
        assert!(parse_status_url("http://example.com:18445/status").is_err());
        assert!(parse_status_url("https://127.0.0.1:18445/status").is_err());
    }
}
