//! CLI command handlers for evo-control.
//!
//! Each function opens the driver (or config store), performs the operation,
//! and prints the result to stdout.

use anyhow::{Context, Result};
use evo_config::DeviceState;
use evo_driver::DriverHandle;

// ── set ───────────────────────────────────────────────────────────────────

pub fn set_volume(handle: &DriverHandle, db: f32, pair: Option<u8>) -> Result<()> {
    match pair {
        Some(p) => {
            let actual = handle.volume_set(p, db)?;
            println!("pair {p} volume set to {actual:.1} dB");
        }
        None => {
            for p in 0..2 {
                let actual = handle.volume_set(p, db)?;
                if p == 0 {
                    println!("volume set to {actual:.1} dB (both pairs)");
                }
            }
        }
    }
    Ok(())
}

pub fn set_gain(handle: &DriverHandle, input: u8, db: f32) -> Result<()> {
    let idx = input.checked_sub(1).context("input must be 1–4")?;
    let actual = handle.gain_set(idx, db)?;
    println!("input {input} gain set to {actual:.1} dB");
    Ok(())
}

pub fn set_phantom(handle: &DriverHandle, input: u8, on: bool) -> Result<()> {
    let idx = input.checked_sub(1).context("input must be 1–4")?;
    handle.phantom_set(idx, on)?;
    println!("input {input} phantom {}", if on { "ON" } else { "OFF" });
    Ok(())
}

pub fn set_mute(handle: &DriverHandle, target: &str, on: bool) -> Result<()> {
    match target {
        t if t.starts_with("input") => {
            let n: u8 = t[5..].parse().context("input must be 1–4")?;
            let idx = n.checked_sub(1).context("input must be 1–4")?;
            handle.input_mute_set(idx, on)?;
            println!("input {n} {}", if on { "muted" } else { "unmuted" });
        }
        "output" => {
            handle.output_mute_set(on)?;
            println!("output {}", if on { "muted" } else { "unmuted" });
        }
        _ => anyhow::bail!("target must be 'input<N>' or 'output'"),
    }
    Ok(())
}

// ── get ───────────────────────────────────────────────────────────────────

pub fn get_control(handle: &DriverHandle, control: &str, target: Option<&str>) -> Result<()> {
    match control {
        "volume" => {
            let db = handle.status()?.volume_db[0];
            println!("{db:.1} dB");
        }
        "gain" => {
            let idx = target
                .and_then(|t| t.parse::<u8>().ok())
                .and_then(|n| n.checked_sub(1))
                .context("usage: evo-control get gain <INPUT>")?;
            let db = handle.status()?.gain_db[idx as usize];
            println!("{db:.1} dB");
        }
        "phantom" => {
            let idx = target
                .and_then(|t| t.parse::<u8>().ok())
                .and_then(|n| n.checked_sub(1))
                .context("usage: evo-control get phantom <INPUT>")?;
            let on = handle.status()?.phantom[idx as usize];
            println!("{}", if on { "ON" } else { "OFF" });
        }
        "mute" => match target.unwrap_or("") {
            t if t.starts_with("input") => {
                let n: u8 = t[5..].parse().context("input must be 1–4")?;
                let idx = n.checked_sub(1).context("input must be 1–4")?;
                let on = handle.status()?.input_mute[idx as usize];
                println!("{}", if on { "muted" } else { "unmuted" });
            }
            "output" => {
                let on = handle.status()?.output_mute;
                println!("{}", if on { "muted" } else { "unmuted" });
            }
            _ => anyhow::bail!("usage: evo-control get mute <inputN|output>"),
        },
        _ => anyhow::bail!("unknown control '{control}'; try: volume, gain, phantom, mute"),
    }
    Ok(())
}

// ── status ────────────────────────────────────────────────────────────────

pub fn status(handle: &DriverHandle) -> Result<()> {
    let s = handle.status()?;
    println!("EVO 8 status");
    println!("────────────");
    for (i, db) in s.volume_db.iter().enumerate() {
        let label = ["Main", "Headphones"][i];
        println!("  {label}: {db:.1} dB");
    }
    println!("  Output mute: {}", if s.output_mute { "muted" } else { "unmuted" });
    for i in 0..4 {
        println!(
            "  Input {}: gain {:.1} dB  phantom {}  {}",
            i + 1,
            s.gain_db[i],
            if s.phantom[i] { "ON " } else { "OFF" },
            if s.input_mute[i] { "muted" } else { "unmuted" },
        );
    }
    Ok(())
}

// ── mixer ─────────────────────────────────────────────────────────────────

pub fn mixer_set(handle: &DriverHandle, in_idx: u8, out_idx: u8, db: f32) -> Result<()> {
    handle.mixer_set(in_idx, out_idx, db)?;
    println!("mixer [{in_idx}][{out_idx}] = {db:.1} dB");
    Ok(())
}

pub fn mixer_get() -> Result<()> {
    let state_file = evo_config::state_file();
    let state = evo_config::store::load_state(&state_file)?
        .unwrap_or_default();

    println!("Mixer matrix (10 inputs × 4 outputs)");
    println!("Values in dB, -128 = silence");
    println!();
    print!("       ");
    for out in 0..4 {
        print!("  out{out:>5} ");
    }
    println!();
    println!("  {}", "─".repeat(50));
    for in_idx in 0..10 {
        let label = if in_idx < 4 {
            format!("in{}  ", in_idx + 1)
        } else if in_idx < 8 {
            format!("DAW{}", in_idx - 3)
        } else {
            format!("LOOP{}", in_idx - 7)
        };
        print!("  {label}  ");
        for out_idx in 0..4 {
            let db = state.mixer[in_idx][out_idx];
            if db <= -128.0 {
                print!("  {:<7}", "—");
            } else {
                print!("  {db:>5.1}  ");
            }
        }
        println!();
    }
    Ok(())
}

pub fn mixer_reset(handle: &DriverHandle) -> Result<()> {
    let defaults = DeviceState::default();
    for in_idx in 0..10 {
        for out_idx in 0..4 {
            let db = defaults.mixer[in_idx][out_idx];
            handle.mixer_set(in_idx as u8, out_idx as u8, db)?;
        }
    }
    // Persist to state.toml
    let saved = evo_config::store::load_state(&evo_config::state_file())?
        .unwrap_or_default();
    let config = evo_config::Config {
        state: DeviceState { mixer: defaults.mixer, ..saved },
        presets: std::collections::HashMap::new(),
        state_path: evo_config::state_file(),
    };
    config.save_state()?;
    println!("mixer reset to defaults");
    Ok(())
}

// ── preset ────────────────────────────────────────────────────────────────

pub fn preset_save(name: &str) -> Result<()> {
    let state = load_current_state()?;
    let mut config = evo_config::Config { state, presets: std::collections::HashMap::new(), state_path: evo_config::state_file() };
    config.save_preset(name)?;
    println!("preset '{name}' saved");
    Ok(())
}

pub fn preset_load(handle: &DriverHandle, name: &str) -> Result<()> {
    // Read the preset file and apply every control to the device
    let file = evo_config::preset_file(name);
    let preset = evo_config::store::load_state(&file)?
        .ok_or_else(|| anyhow::anyhow!("preset '{name}' not found"))?;

    apply_preset(handle, &preset)?;
    // Persist to state.toml as the new current state
    let config = evo_config::Config { state: preset, presets: std::collections::HashMap::new(), state_path: evo_config::state_file() };
    config.save_state()?;
    println!("preset '{name}' loaded");
    Ok(())
}

pub fn preset_list() -> Result<()> {
    let dir = evo_config::presets_dir();
    let names = evo_config::store::list_presets(&dir)?;
    if names.is_empty() {
        println!("no presets saved");
    } else {
        for name in &names {
            println!("  {name}");
        }
    }
    Ok(())
}

pub fn preset_delete(name: &str) -> Result<()> {
    let file = evo_config::preset_file(name);
    evo_config::store::delete_preset(&file)?;
    println!("preset '{name}' deleted");
    Ok(())
}

pub fn apply_default_preset(handle: &DriverHandle) -> Result<()> {
    let state_path = evo_config::state_file();
    let state = evo_config::store::load_state(&state_path)?
        .unwrap_or_default();
    apply_preset(handle, &state)?;
    println!("default preset applied");
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────

fn load_current_state() -> Result<DeviceState> {
    // Merge driver-readable state with the on-disk shadow (for MU60).
    // For now, if the driver isn't available, return a default.
    let (handle, _disc) = match evo_driver::worker::spawn() {
        Ok(h) => h,
        Err(_) => return Ok(DeviceState::default()),
    };
    let driver_status = handle.status().unwrap_or_default();
    let mut state = DeviceState::default();
    state.output_volume_db = driver_status.volume_db;
    state.input_gain_db = driver_status.gain_db;
    state.phantom = driver_status.phantom;
    state.input_mute = driver_status.input_mute;
    state.output_mute = driver_status.output_mute;
    // Merge from existing state.toml to preserve mixer shadow
    if let Ok(Some(existing)) = evo_config::store::load_state(&evo_config::state_file()) {
        state.mixer = existing.mixer;
    }
    handle.shutdown();
    Ok(state)
}

fn apply_preset(handle: &DriverHandle, preset: &DeviceState) -> Result<()> {
    handle.volume_set(0, preset.output_volume_db[0])?;
    handle.volume_set(1, preset.output_volume_db[1])?;
    for i in 0..4 {
        handle.gain_set(i as u8, preset.input_gain_db[i])?;
        handle.phantom_set(i as u8, preset.phantom[i])?;
        handle.input_mute_set(i as u8, preset.input_mute[i])?;
    }
    handle.output_mute_set(preset.output_mute)?;
    // MU60 mixer cross-points (write-only)
    for in_idx in 0..10 {
        for out_idx in 0..4 {
            let db = preset.mixer[in_idx][out_idx];
            handle.mixer_set(in_idx as u8, out_idx as u8, db)?;
        }
    }
    Ok(())
}
