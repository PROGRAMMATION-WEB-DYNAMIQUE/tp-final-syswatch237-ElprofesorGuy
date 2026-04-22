// src/main.rs
use chrono::Local;
use std::fmt;
use sysinfo::{System, Process};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::fs::OpenOptions;

const AUTH_TOKEN: &str = "ENSPD2025";

// --- Modélisation des données (Étape 1) ---

#[derive(Debug, Clone)]
struct CpuInfo {
    usage_percent: f32,
    core_count: usize,
}

#[derive(Debug, Clone)]
struct MemInfo {
    total_mb: u64,
    used_mb: u64,
    free_mb: u64,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory_mb: u64,
}

#[derive(Debug, Clone)]
struct NetworkInfo {
    interface_name: String,
    bytes_received: u64,
    bytes_transmitted: u64,
}

#[derive(Debug, Clone)]
struct AlertStatus {
    cpu_critical: bool,
    #[allow(dead_code)]
    cpu_warning: bool,
    memory_critical: bool,
    #[allow(dead_code)]
    memory_warning: bool,
    alerts: Vec<String>,
}

#[derive(Debug, Clone)]
struct SystemSnapshot {
    timestamp: String,
    cpu: CpuInfo,
    memory: MemInfo,
    top_processes: Vec<ProcessInfo>,
    network: Vec<NetworkInfo>,
    alerts: AlertStatus,
}

// --- Implémentation du trait Display pour chaque type (Étape 1) ---

impl fmt::Display for CpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CPU: {:.1}% ({} cœurs)", self.usage_percent, self.core_count)
    }
}

impl fmt::Display for MemInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MEM: {}MB utilisés / {}MB total ({} MB libres)",
            self.used_mb, self.total_mb, self.free_mb
        )
    }
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  [{:>6}] {:<25} CPU:{:>5.1}%  MEM:{:>5}MB",
            self.pid, self.name, self.cpu_usage, self.memory_mb
        )
    }
}

impl fmt::Display for NetworkInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rx_mb = self.bytes_received / 1024 / 1024;
        let tx_mb = self.bytes_transmitted / 1024 / 1024;
        write!(f, "  {} — RX: {}MB | TX: {}MB", self.interface_name, rx_mb, tx_mb)
    }
}

impl fmt::Display for AlertStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.alerts.is_empty() {
            write!(f, "✓ Système nominal — Aucune alerte")
        } else {
            let level = if self.cpu_critical || self.memory_critical {
                " CRITIQUE"
            } else {
                " ATTENTION"
            };
            write!(f, "{} — {} alerte(s) active(s)", level, self.alerts.len())
        }
    }
}

impl fmt::Display for SystemSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== SysWatch — {} ===", self.timestamp)?;
        writeln!(f, "{}", self.cpu)?;
        writeln!(f, "{}", self.memory)?;
        writeln!(f, "--- Alertes ---")?;
        writeln!(f, "{}", self.alerts)?;
        writeln!(f, "--- Top Processus ---")?;
        for p in &self.top_processes {
            writeln!(f, "{}", p)?;
        }
        writeln!(f, "--- Réseau ---")?;
        for n in &self.network {
            writeln!(f, "{}", n)?;
        }
        write!(f, "=====================")
    }
}

// --- Gestion d'erreurs personnalisée (Étape 2) ---

#[derive(Debug)]
enum SysWatchError {
    CollectionFailed(String),
}

impl fmt::Display for SysWatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysWatchError::CollectionFailed(msg) => write!(f, "Erreur collecte: {}", msg),
        }
    }
}

impl std::error::Error for SysWatchError {}

// --- Collecte des informations système (Étape 2) ---

fn collect_network_info() -> Vec<NetworkInfo> {
    // Note: sysinfo 0.30 n'expose pas facilement les statistiques réseau
    // Nous simulons des données cohérentes pour la démonstration
    // En production, on pourrait utiliser des crates supplémentaires (pnet, etc.)
    vec![
        NetworkInfo {
            interface_name: "Ethernet".to_string(),
            bytes_received: 1024 * 1024 * 100,  // 100 MB
            bytes_transmitted: 1024 * 1024 * 50, // 50 MB
        },
        NetworkInfo {
            interface_name: "WiFi".to_string(),
            bytes_received: 1024 * 1024 * 250, // 250 MB
            bytes_transmitted: 1024 * 1024 * 150, // 150 MB
        },
    ]
}

fn check_alerts(cpu_usage: f32, used_mb: u64, total_mb: u64) -> AlertStatus {
    let mem_percent = (used_mb as f64 / total_mb as f64) * 100.0;
    let mut alerts = Vec::new();

    let cpu_critical = cpu_usage > 85.0;
    let cpu_warning = cpu_usage > 70.0 && cpu_usage <= 85.0;
    let memory_critical = mem_percent > 90.0;
    let memory_warning = mem_percent > 75.0 && mem_percent <= 90.0;

    if cpu_critical {
        alerts.push(format!("CPU CRITIQUE: {:.1}%", cpu_usage));
    } else if cpu_warning {
        alerts.push(format!("CPU ÉLEVÉ: {:.1}%", cpu_usage));
    }

    if memory_critical {
        alerts.push(format!("RAM CRITIQUE: {:.1}%", mem_percent));
    } else if memory_warning {
        alerts.push(format!("RAM ÉLEVÉE: {:.1}%", mem_percent));
    }

    AlertStatus {
        cpu_critical,
        cpu_warning,
        memory_critical,
        memory_warning,
        alerts,
    }
}

// --- Collecte système complète avec gestion d'erreurs (Étape 2) ---

fn collect_snapshot() -> Result<SystemSnapshot, SysWatchError> {
    let mut sys = System::new_all();
    sys.refresh_all();

    // Petite pause pour que sysinfo ait des valeurs CPU non nulles
    std::thread::sleep(std::time::Duration::from_millis(500));
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_info().cpu_usage();
    let core_count = sys.cpus().len();

    if core_count == 0 {
        return Err(SysWatchError::CollectionFailed("Aucun CPU détecté".to_string()));
    }

    let total_mb = sys.total_memory() / 1024 / 1024;
    let used_mb = sys.used_memory() / 1024 / 1024;
    let free_mb = sys.free_memory() / 1024 / 1024;

    // Top 5 processus par consommation CPU
    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .values()
        .map(|p: &Process| ProcessInfo {
            pid: p.pid().as_u32(),
            name: p.name().to_string(),
            cpu_usage: p.cpu_usage(),
            memory_mb: p.memory() / 1024 / 1024,
        })
        .collect();

    processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap());
    processes.truncate(5);

    // Nouvelles données : réseau et alertes
    let network = collect_network_info();
    let alerts = check_alerts(cpu_usage, used_mb, total_mb);

    Ok(SystemSnapshot {
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        cpu: CpuInfo { usage_percent: cpu_usage, core_count },
        memory: MemInfo { total_mb, used_mb, free_mb },
        top_processes: processes,
        network,
        alerts,
    })
}

// --- Formatage des réponses (Étape 3) ---

fn format_response(snapshot: &SystemSnapshot, command: &str) -> String {
    let cmd = command.trim().to_lowercase();

    match cmd.as_str() {
        "cpu" => format!(
            "[CPU]\n{}\n\nHistorique:\n{}\n",
            snapshot.cpu,
            // Itérateur : simuler une barre de progression ASCII
            (0..10)
                .map(|i| {
                    let threshold = (snapshot.cpu.usage_percent / 10.0) as usize;
                    if i < threshold { "█" } else { "░" }
                })
                .collect::<Vec<_>>()
                .join("") + &format!(" {:.1}%", snapshot.cpu.usage_percent)
        ),

        "mem" => {
            let percent = (snapshot.memory.used_mb as f64 / snapshot.memory.total_mb as f64) * 100.0;
            let bar: String = (0..20)
                .map(|i| if i < (percent / 5.0) as usize { '█' } else { '░' })
                .collect();
            format!(
                "[MÉMOIRE]\n{}\n[{}] {:.1}%\n",
                snapshot.memory, bar, percent
            )
        },

        "ps" | "procs" => {
            let lines: String = snapshot
                .top_processes
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}. {}", i + 1, p))
                .collect::<Vec<_>>()
                .join("\n");
            format!("[PROCESSUS — Top {}]\n{}\n", snapshot.top_processes.len(), lines)
        },

        "shutdown" => {
            // Windows
            std::process::Command::new("shutdown")
                .args(["/s", "/t", "5"])
                .spawn()
                .ok();
            "SHUTDOWN programmé dans 5 secondes.\n".to_string()
        }

        "reboot" => {
            std::process::Command::new("shutdown")
                .args(["/r", "/t", "5"])
                .spawn()
                .ok();
            "REBOOT programmé dans 5 secondes.\n".to_string()
        }

        "abort" => {
            // Annuler un shutdown/reboot en cours
            std::process::Command::new("shutdown")
                .args(["/a"])
                .spawn()
                .ok();
            "Extinction annulée.\n".to_string()
        }

        _ if cmd.starts_with("msg ") => {
            // Afficher un message dans le terminal de l'étudiant
            // msg Bonjour tout le monde !
            let text = &cmd[4..];
            println!("\n╔══════════════════════════════════════╗");
            println!("║  MESSAGE DU PROFESSEUR               ║");
            println!("║  {}{}║", text, " ".repeat(38usize.saturating_sub(text.len())));
            println!("╚══════════════════════════════════════╝\n");
            format!("Message affiché sur la machine cible.\n")
        }

        _ if cmd.starts_with("install ") => {
            // install <nom-du-package-winget>
            // ex: install git.git
            let package = cmd[8..].trim().to_string();
            std::thread::spawn(move || {
                std::process::Command::new("winget")
                    .args(["install", "--silent", &package])
                    .status()
                    .ok();
            });
            format!("Installation de '{}' lancée en arrière-plan.\n", &cmd[8..])
        }

        "net" => {
            let lines: String = snapshot
                .network
                .iter()
                .map(|n| format!("{}", n))
                .collect::<Vec<_>>()
                .join("\n");
            if lines.is_empty() {
                "[RÉSEAU]\nAucune interface détectée.\n".to_string()
            } else {
                format!("[RÉSEAU]\n{}\n", lines)
            }
        }

        "alert" | "alerts" => {
            let alert_details: String = snapshot
                .alerts
                .alerts
                .iter()
                .map(|a| format!("  {}\n", a))
                .collect();
            format!(
                "[ALERTES]\n{}\n{}\n",
                snapshot.alerts,
                if alert_details.is_empty() {
                    "Aucune alerte — Tout va bien ! 👍".to_string()
                } else {
                    format!("\nDétails:\n{}", alert_details)
                }
            )
        }

        "all" | "" => format!("{}\n", snapshot),

        "help" => concat!(
            "Commandes disponibles:\n",
            "  cpu   — Usage CPU + barre\n",
            "  mem   — Mémoire RAM\n",
            "  ps    — Top processus\n",
            "  net   — Interfaces réseau 🌐\n",
            "  alert — État des alertes ⚠️\n",
            "  all   — Vue complète\n",
            "  help  — Cette aide\n",
            "  quit  — Fermer la connexion\n",
        ).to_string(),

        "quit" | "exit" => "BYE\n".to_string(),

        _ => format!("Commande inconnue: '{}'. Tape 'help'.\n", command.trim()),
    }
}


// --- Actualisation du snapshot toutes les 5 secondes (Étape 4) ---

fn snapshot_refresher(snapshot: Arc<Mutex<SystemSnapshot>>) {
    loop {
        thread::sleep(Duration::from_secs(5));
        match collect_snapshot() {
            Ok(new_snap) => {
                let mut snap = snapshot.lock().unwrap();
                *snap = new_snap;
                println!("[refresh] Métriques mises à jour");
            }
            Err(e) => eprintln!("[refresh] Erreur: {}", e),
        }
    }
}


// --- Journalisation avec horodatage (Étape 5) ---

fn log_event(message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{}] {}\n", timestamp, message);

    // Écriture console
    print!("{}", line);

    // Écriture fichier — on ignore l'erreur silencieusement (best-effort)
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("syswatch.log")
    {
        let _ = file.write_all(line.as_bytes());
    }
}


// --- Gestion des clients TCP avec authentification (Étape 4) ---

fn handle_client(mut stream: TcpStream, snapshot: Arc<Mutex<SystemSnapshot>>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or("inconnu".to_string());
    log_event(&format!("[+] Connexion de {}", peer));

    // Étape 1 : demander le token
    let _ = stream.write_all(b"TOKEN:\n");
    stream.flush().ok();
    let mut reader = BufReader::new(stream.try_clone().expect("Clone failed"));
    let mut token_line = String::new();
    if reader.read_line(&mut token_line).is_err() {
        let _ = stream.write_all(b"UNAUTHORIZED\n");
        log_event(&format!("[!] Accès refusé depuis {}", peer));
        return;
    }
    let trimmed = token_line.trim().replace('\0', "");
    log_event(&format!("Token reçu: '{:?}'", trimmed));
    if !trimmed.eq_ignore_ascii_case(AUTH_TOKEN) {
        let _ = stream.write_all(b"UNAUTHORIZED\n");
        log_event(&format!("[!] Accès refusé depuis {}", peer));
        return;
    }
    let _ = stream.write_all(b"OK\n");
    log_event(&format!("[✓] Authentifié: {}", peer));

    // Boucle de commandes
    for line in reader.lines() {
        match line {
            Ok(cmd) => {
                let cmd = cmd.trim().to_string();
                log_event(&format!("[{}] commande: '{}'", peer, cmd));

                if cmd.eq_ignore_ascii_case("quit") {
                    let _ = stream.write_all(b"BYE\n");
                    break;
                }

                let response = {
                    let snap = snapshot.lock().unwrap();
                    format_response(&snap, &cmd)
                };

                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(b"\nEND\n"); // marqueur fin de réponse
            }
            Err(_) => break,
        }
    }

    log_event(&format!("[-] Déconnexion de {}", peer));
}

// --- Point d'entrée principal (Étape 4) ---
// Serveur TCP complet avec authentification, rafraîchissement automatique et journalisation

fn main() {
    println!("SysWatch démarrage...");

    // Collecte initiale
    let initial = collect_snapshot().expect("Impossible de collecter les métriques initiales");
    println!("Métriques initiales OK:\n{}", initial);

    // Snapshot partagé entre tous les threads
    let shared_snapshot = Arc::new(Mutex::new(initial));

    // Thread de rafraîchissement automatique toutes les 5s
    {
        let snap_clone = Arc::clone(&shared_snapshot);
        thread::spawn(move || snapshot_refresher(snap_clone));
    }

    // Démarrage du serveur TCP
    let listener = TcpListener::bind("0.0.0.0:7878").expect("Impossible de bind le port 7878");
    println!("Serveur en écoute sur port 7878...");
    println!("Connecte-toi avec: telnet localhost 7878");
    println!("  ou: nc localhost 7878 (WSL/Git Bash)");
    println!("  ou: Test-NetConnection localhost -Port 7878 (PowerShell - test seulement)");
    println!("Ctrl+C pour arrêter.\n");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let snap_clone = Arc::clone(&shared_snapshot);
                thread::spawn(move || handle_client(stream, snap_clone));
            }
            Err(e) => eprintln!("Erreur connexion entrante: {}", e),
        }
    }
}