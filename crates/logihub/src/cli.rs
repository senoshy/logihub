//! Command-line interface for LogiHub.

use clap::{Parser, Subcommand, ValueEnum};
use hidpp::{LogitechMouse, MAX_DPI_STAGES, OnboardMode, feature_name};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "logihub",
    version,
    about = "Native Linux Logitech G Hub alternative (HID++ 2.0 GUI & CLI)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the native desktop GUI (default when no subcommand is given)
    Gui,

    /// Show connected mouse status (model, battery, DPI, polling rate, onboard mode)
    Status {
        /// Output status as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show battery percentage and charging status
    Battery,

    /// Set active DPI or configure all onboard DPI stages
    Dpi {
        /// Single active DPI value to apply (e.g. 800)
        #[arg(value_name = "DPI")]
        value: Option<u16>,

        /// Comma-separated list of up to 5 onboard DPI stages (e.g. 400,800,1600,3200)
        #[arg(long, value_delimiter = ',')]
        stages: Option<Vec<u16>>,

        /// Active stage index (0-based) when using --stages
        #[arg(long, default_value_t = 0)]
        active: usize,

        /// Apply live only without writing to onboard flash memory
        #[arg(long)]
        no_persist: bool,
    },

    /// Set mouse polling rate in Hz (e.g. 125, 250, 500, 1000, 2000, 4000, 8000)
    Rate {
        /// Target polling rate in Hz
        hz: u16,

        /// Apply live only without writing to onboard flash memory
        #[arg(long)]
        no_persist: bool,
    },

    /// Switch between onboard memory mode and host (software) mode
    Mode {
        #[arg(value_enum)]
        mode: ModeArg,
    },

    /// Dump all discovered HID++ 2.0 features and onboard memory geometry
    Inspect {
        /// Also hex-dump the active onboard profile sector (read-only)
        #[arg(long)]
        dump_profile: bool,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ModeArg {
    Onboard,
    Host,
}

#[derive(Serialize)]
struct JsonStatus {
    hidraw_path: String,
    vendor_id: String,
    product_id: String,
    device_index: u8,
    protocol_version: String,
    name: String,
    battery_percent: Option<u8>,
    battery_charging: Option<String>,
    battery_voltage_mv: Option<u16>,
    dpi_current: Option<u16>,
    dpi_default: Option<u16>,
    report_rate_hz: Option<u16>,
    supported_rates_hz: Vec<u16>,
    onboard_mode: Option<String>,
    active_profile_sector: Option<u16>,
    onboard_dpi_stages: Option<[u16; MAX_DPI_STAGES]>,
    onboard_dpi_index: Option<usize>,
    onboard_report_rate_hz: Option<u16>,
}

pub fn run_command(cmd: Command) -> Result<(), String> {
    let mut mouse = LogitechMouse::connect_first().map_err(|e| e.to_string())?;

    match cmd {
        Command::Gui => unreachable!("GUI is dispatched in main"),
        Command::Status { json } => {
            let status = mouse.status().map_err(|e| e.to_string())?;
            if json {
                let payload = JsonStatus {
                    hidraw_path: status.hidraw_path.display().to_string(),
                    vendor_id: format!("0x{:04X}", status.vendor_id),
                    product_id: format!("0x{:04X}", status.product_id),
                    device_index: status.device_index,
                    protocol_version: format!(
                        "{}.{}",
                        status.protocol_version.0, status.protocol_version.1
                    ),
                    name: status.name,
                    battery_percent: status.battery.as_ref().map(|b| b.percentage),
                    battery_charging: status.battery.as_ref().map(|b| b.charging.to_string()),
                    battery_voltage_mv: status.battery.as_ref().and_then(|b| b.voltage_mv),
                    dpi_current: status.dpi.map(|d| d.current_x),
                    dpi_default: status.dpi.map(|d| d.default_dpi),
                    report_rate_hz: status.report_rate_hz,
                    supported_rates_hz: status.supported_rates_hz,
                    onboard_mode: status.onboard_mode.map(|m| m.to_string()),
                    active_profile_sector: status.active_profile_sector,
                    onboard_dpi_stages: status.onboard_dpi_stages,
                    onboard_dpi_index: status.onboard_dpi_index,
                    onboard_report_rate_hz: status.onboard_report_rate_hz,
                };
                let out = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
                println!("{out}");
            } else {
                println!("Device:        {}", status.name);
                println!(
                    "Endpoint:      {} (VID:PID {:04X}:{:04X}, idx 0x{:02X}, HID++ {}.{})",
                    status.hidraw_path.display(),
                    status.vendor_id,
                    status.product_id,
                    status.device_index,
                    status.protocol_version.0,
                    status.protocol_version.1
                );
                if let Some(batt) = status.battery {
                    match batt.voltage_mv {
                        Some(mv) => {
                            println!("Battery:       {}% ({}, {} mV)", batt.percentage, batt.charging, mv)
                        }
                        None => println!("Battery:       {}% ({})", batt.percentage, batt.charging),
                    }
                }
                if let Some(dpi) = status.dpi {
                    if dpi.current_x == dpi.current_y {
                        println!("DPI (live):    {} DPI", dpi.current_x);
                    } else {
                        println!("DPI (live):    {} × {} DPI", dpi.current_x, dpi.current_y);
                    }
                }
                if let Some(rate) = status.report_rate_hz {
                    println!(
                        "Polling Rate:  {} Hz (supported: {:?})",
                        rate, status.supported_rates_hz
                    );
                }
                if let Some(mode) = status.onboard_mode {
                    println!("Memory Mode:   {mode}");
                }
                if let Some(stages) = status.onboard_dpi_stages {
                    let active = status.onboard_dpi_index.unwrap_or(0);
                    let formatted: Vec<String> = stages
                        .iter()
                        .enumerate()
                        .filter(|(_, d)| **d > 0)
                        .map(|(i, d)| {
                            if i == active {
                                format!("[{d}]")
                            } else {
                                d.to_string()
                            }
                        })
                        .collect();
                    println!("Onboard DPI:   {}", formatted.join("  "));
                }
                if let Some(rate) = status.onboard_report_rate_hz {
                    println!("Onboard Rate:  {rate} Hz");
                }
            }
            Ok(())
        }
        Command::Battery => {
            let status = mouse.status().map_err(|e| e.to_string())?;
            match status.battery {
                Some(batt) => {
                    println!("{}% ({})", batt.percentage, batt.charging);
                    Ok(())
                }
                None => Err("battery feature not supported on this device".to_string()),
            }
        }
        Command::Dpi {
            value,
            stages,
            active,
            no_persist,
        } => {
            if let Some(stage_list) = stages {
                if stage_list.is_empty() || stage_list.len() > MAX_DPI_STAGES {
                    return Err(format!(
                        "--stages expects between 1 and {MAX_DPI_STAGES} DPI values"
                    ));
                }
                let mut arr = [0u16; MAX_DPI_STAGES];
                for (i, &val) in stage_list.iter().enumerate() {
                    arr[i] = val;
                }
                let applied = mouse
                    .set_dpi_stages(arr, active)
                    .map_err(|e| e.to_string())?;
                println!(
                    "Onboard DPI stages updated: {:?} (active index {}, {} DPI)",
                    applied, active, applied[active]
                );
                Ok(())
            } else if let Some(dpi) = value {
                let applied = mouse
                    .set_dpi(dpi, !no_persist)
                    .map_err(|e| e.to_string())?;
                if no_persist {
                    println!("Set live DPI to {applied} (not persisted to onboard memory)");
                } else {
                    println!("Set DPI to {applied} (applied live + persisted to onboard memory)");
                }
                Ok(())
            } else {
                Err("provide a DPI value (e.g. `logihub dpi 800`) or `--stages 400,800,1600`".to_string())
            }
        }
        Command::Rate { hz, no_persist } => {
            mouse
                .set_report_rate(hz, !no_persist)
                .map_err(|e| e.to_string())?;
            if no_persist {
                println!("Set live polling rate to {hz} Hz (not persisted to onboard memory)");
            } else {
                println!("Set polling rate to {hz} Hz (applied live + persisted to onboard memory)");
            }
            Ok(())
        }
        Command::Mode { mode } => {
            let target = match mode {
                ModeArg::Onboard => OnboardMode::Onboard,
                ModeArg::Host => OnboardMode::Host,
            };
            mouse.set_onboard_mode(target).map_err(|e| e.to_string())?;
            println!("Switched memory mode to {target}");
            Ok(())
        }
        Command::Inspect { dump_profile } => {
            let status = mouse.status().map_err(|e| e.to_string())?;
            println!(
                "Device: {} ({}, VID:PID {:04X}:{:04X}, dev_idx 0x{:02X}, HID++ {}.{})",
                status.name,
                status.hidraw_path.display(),
                status.vendor_id,
                status.product_id,
                status.device_index,
                status.protocol_version.0,
                status.protocol_version.1
            );
            let features = mouse.inspect_features().map_err(|e| e.to_string())?;
            println!("\nDiscovered HID++ 2.0 features ({}):", features.len());
            for f in features {
                println!(
                    "  [{:02}] 0x{:04X}  v{}  type=0x{:02X}  {}",
                    f.index,
                    f.id,
                    f.version,
                    f.feature_type,
                    feature_name(f.id)
                );
            }
            if let Ok(desc) = mouse.onboard_description() {
                println!("\nOnboard Profiles (0x8100) geometry:");
                println!("  memory_model_id:   0x{:02X}", desc.memory_model_id);
                println!("  profile_format_id: 0x{:02X}", desc.profile_format_id);
                println!("  macro_format_id:   0x{:02X}", desc.macro_format_id);
                println!("  profile_count:     {}", desc.profile_count);
                println!("  button_count:      {}", desc.button_count);
                println!("  sector_count:      {}", desc.sector_count);
                println!("  sector_size:       {} bytes", desc.sector_size);
                println!("  active_sector:     {:?}", status.active_profile_sector);

                if dump_profile && let Some(sector_id) = status.active_profile_sector {
                    match hidpp::onboard::read_sector(
                        mouse.session_mut(),
                        sector_id,
                        desc.sector_size,
                    ) {
                        Ok(raw) => {
                            println!(
                                "\nActive profile sector 0x{:04X} ({} bytes):",
                                sector_id,
                                raw.len()
                            );
                            for (row, chunk) in raw.chunks(16).enumerate() {
                                print!("  {:04X}: ", row * 16);
                                for b in chunk {
                                    print!("{b:02X} ");
                                }
                                println!();
                            }
                            println!("  CRC check: {:?}", hidpp::crc::verify_sector_crc(&raw));
                        }
                        Err(err) => {
                            eprintln!("  read_sector(0x{sector_id:04X}) failed: {err}");
                        }
                    }
                }
            }
            Ok(())
        }
    }
}
