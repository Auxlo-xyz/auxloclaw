//! Status command handler

use anyhow::Result;
use sysinfo::System;

pub fn handle_status() -> Result<()> {
    println!("\n🦞 AUXLOCLAW Status\n");
    
    // System info
    let mut sys = System::new_all();
    sys.refresh_all();
    println!("📊 System");
    let os_name = System::name().unwrap_or_default();
    println!("  OS: {}", os_name);
    println!("  CPU: {} cores", sys.cpus().len());
    println!("  Memory: {}/{} MB", 
        sys.used_memory() / 1024 / 1024,
        sys.total_memory() / 1024 / 1024
    );
    
    // Config
    println!("\n⚙️ Configuration");
    let config_dir = dirs::home_dir()
        .map(|h| h.join(".auxloclaw"))
        .unwrap_or_default();
    
    if config_dir.exists() {
        println!("  Config directory: {}", config_dir.display());
        
        // Check components
        let config_path = config_dir.join("config.toml");
        if config_path.exists() {
            println!("  Config file: ✓");
        } else {
            println!("  Config file: ✗ (run `auxloclaw setup`)");
        }
        
        let skills_dir = config_dir.join("skills");
        if skills_dir.exists() {
            let skill_count = walkdir::WalkDir::new(&skills_dir)
                .min_depth(2)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name() == "SKILL.md")
                .count();
            println!("  Skills: {} installed", skill_count);
        }
        
        let memory_dir = config_dir.join("memory");
        if memory_dir.exists() {
            println!("  Memory: ✓");
        }
    } else {
        println!("  Not configured (run `auxloclaw setup`)");
    }
    
    // Running processes
    println!("\n🔄 Running Instances");
    let mut found = false;
    for (pid, process) in sys.processes() {
        if process.name().contains("auxloclaw") {
            println!("  PID: {}", pid);
            println!("  Memory: {} KB", process.memory());
            println!("  CPU: {:.1}%", process.cpu_usage());
            found = true;
        }
    }
    
    if !found {
        println!("  No running instances");
    }
    
    // Network - use std net instead of blocking reqwest
    println!("\n🌐 Network");
    use std::net::TcpStream;
    match TcpStream::connect("127.0.0.1:18789") {
        Ok(_) => println!("  Gateway: ✓ (port 18789)"),
        Err(_) => println!("  Gateway: not running"),
    }
    
    println!();
    Ok(())
}