//! Combinaisons de touches : "ctrl+alt+p" <-> (modificateurs, code VK).
//! Les noms suivent ceux de la lib Python `keyboard` pour rester
//! compatibles avec les config.json existants.

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

/// (code VK, nom canonique, alias)
const KEYS: &[(u32, &str, &[&str])] = &[
    (0x08, "backspace", &[]),
    (0x09, "tab", &[]),
    (0x0D, "enter", &["return"]),
    (0x13, "pause", &["break"]),
    (0x14, "caps lock", &[]),
    (0x1B, "esc", &["escape"]),
    (0x20, "space", &["spacebar"]),
    (0x21, "page up", &["pageup"]),
    (0x22, "page down", &["pagedown"]),
    (0x23, "end", &[]),
    (0x24, "home", &[]),
    (0x25, "left", &[]),
    (0x26, "up", &[]),
    (0x27, "right", &[]),
    (0x28, "down", &[]),
    (0x2C, "print screen", &["printscreen", "snapshot"]),
    (0x2D, "insert", &["ins"]),
    (0x2E, "delete", &["del"]),
    (0x5D, "menu", &["apps"]),
    (0x6A, "num *", &[]),
    (0x6B, "num plus", &[]),
    (0x6D, "num -", &[]),
    (0x6E, "num .", &[]),
    (0x6F, "num /", &[]),
    (0x90, "num lock", &[]),
    (0x91, "scroll lock", &[]),
    (0xAD, "volume mute", &[]),
    (0xAE, "volume down", &[]),
    (0xAF, "volume up", &[]),
    (0xB0, "next track", &[]),
    (0xB1, "previous track", &[]),
    (0xB2, "stop media", &[]),
    (0xB3, "play/pause media", &[]),
    (0xBA, ";", &[]),
    (0xBB, "=", &[]),
    (0xBC, ",", &[]),
    (0xBD, "-", &[]),
    (0xBE, ".", &[]),
    (0xBF, "/", &[]),
    (0xC0, "`", &[]),
    (0xDB, "[", &[]),
    (0xDC, "\\", &[]),
    (0xDD, "]", &[]),
    (0xDE, "'", &[]),
];

fn modifier_of(name: &str) -> Option<u32> {
    Some(match name {
        "ctrl" | "control" | "left ctrl" | "right ctrl" => MOD_CONTROL,
        "alt" | "left alt" | "right alt" | "alt gr" => MOD_ALT,
        "shift" | "left shift" | "right shift" => MOD_SHIFT,
        "windows" | "win" | "left windows" | "right windows" | "cmd" | "command" | "super" => MOD_WIN,
        _ => return None,
    })
}

/// Modificateur correspondant à un code VK (pour l'enregistreur).
pub fn vk_modifier(vk: u32) -> Option<u32> {
    Some(match vk {
        0x10 | 0xA0 | 0xA1 => MOD_SHIFT,
        0x11 | 0xA2 | 0xA3 => MOD_CONTROL,
        0x12 | 0xA4 | 0xA5 => MOD_ALT,
        0x5B | 0x5C => MOD_WIN,
        _ => return None,
    })
}

pub fn vk_from_name(name: &str) -> Option<u32> {
    let n = name.trim().to_lowercase();
    if n.chars().count() == 1 {
        let c = n.chars().next().unwrap();
        if c.is_ascii_lowercase() {
            return Some(c.to_ascii_uppercase() as u32);
        }
        if c.is_ascii_digit() {
            return Some(c as u32);
        }
    }
    if let Some(num) = n.strip_prefix('f').and_then(|s| s.parse::<u32>().ok()) {
        if (1..=24).contains(&num) {
            return Some(0x70 + num - 1);
        }
    }
    if let Some(num) = n.strip_prefix("num ").and_then(|s| s.parse::<u32>().ok()) {
        if num <= 9 {
            return Some(0x60 + num);
        }
    }
    if let Some(hex) = n.strip_prefix("vk") {
        return u32::from_str_radix(hex.trim_start_matches("0x"), 16).ok();
    }
    KEYS.iter()
        .find(|(_, canon, aliases)| *canon == n || aliases.contains(&n.as_str()))
        .map(|(vk, _, _)| *vk)
}

pub fn vk_name(vk: u32) -> String {
    match vk {
        0x41..=0x5A => ((vk as u8) as char).to_ascii_lowercase().to_string(),
        0x30..=0x39 => ((vk as u8) as char).to_string(),
        0x70..=0x87 => format!("f{}", vk - 0x70 + 1),
        0x60..=0x69 => format!("num {}", vk - 0x60),
        _ => KEYS
            .iter()
            .find(|(k, _, _)| *k == vk)
            .map(|(_, n, _)| n.to_string())
            .unwrap_or_else(|| format!("vk{vk:02x}")),
    }
}

/// "ctrl+alt+p" -> (MOD_CONTROL|MOD_ALT, 'P'). Une seule touche non-modificatrice.
pub fn parse_combo(combo: &str) -> Result<(u32, u32), String> {
    let mut mods = 0;
    let mut key = None;
    // "num plus" est le seul nom de touche contenant "+", on découpe proprement.
    for part in combo.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        if let Some(m) = modifier_of(&part) {
            mods |= m;
        } else if key.is_some() {
            return Err("une seule touche (hors Ctrl/Alt/Maj/Win) par raccourci".into());
        } else {
            key = Some(vk_from_name(&part).ok_or_else(|| format!("touche inconnue « {part} »"))?);
        }
    }
    let key = key.ok_or("il manque une touche principale (ex. ctrl+alt+p)")?;
    Ok((mods, key))
}

/// Forme canonique d'une combinaison : modificateurs ordonnés puis touche.
pub fn format_combo(mods: u32, vk: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods & MOD_CONTROL != 0 { parts.push("ctrl".into()); }
    if mods & MOD_ALT != 0 { parts.push("alt".into()); }
    if mods & MOD_SHIFT != 0 { parts.push("shift".into()); }
    if mods & MOD_WIN != 0 { parts.push("windows".into()); }
    parts.push(vk_name(vk));
    parts.join("+")
}

/// Affichage lisible : "Ctrl + Alt + P".
pub fn pretty(combo: &str) -> String {
    combo
        .split('+')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        assert_eq!(parse_combo("alt+ctrl+p").unwrap(), (MOD_ALT | MOD_CONTROL, 0x50));
        assert_eq!(parse_combo("ctrl+shift+f12").unwrap(), (MOD_CONTROL | MOD_SHIFT, 0x7B));
        assert_eq!(format_combo(MOD_CONTROL | MOD_ALT, 0x57), "ctrl+alt+w");
        assert!(parse_combo("a+b").is_err());
        assert!(parse_combo("ctrl").is_err());
        assert_eq!(parse_combo("pause").unwrap(), (0, 0x13));
    }
}
