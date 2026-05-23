/// Разбор rusEFI signature (как `SignatureHelper.parse` в Java Console).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RusEfiSignature {
    pub branch: String,
    pub year: String,
    pub month: String,
    pub day: String,
    pub bundle_target: String,
    pub hash: String,
}

const PREFIX: &str = "rusEFI ";

pub fn parse_rusefi_signature(signature: &str) -> Option<RusEfiSignature> {
    let rest = signature.strip_prefix(PREFIX)?.trim();
    let elements: Vec<&str> = rest.split('.').collect();
    if elements.len() != 6 {
        return None;
    }
    Some(RusEfiSignature {
        branch: elements[0].to_string(),
        year: elements[1].to_string(),
        month: elements[2].to_string(),
        day: elements[3].to_string(),
        bundle_target: elements[4].to_string(),
        hash: elements[5].to_string(),
    })
}

/// URL и имя файла кэша `{hash}.ini` для загрузки с rusefi.com.
pub fn ini_download_target(signature: &str) -> Option<(String, String)> {
    let parsed = parse_rusefi_signature(signature)?;
    let file_name = format!("{}.ini", parsed.hash);
    let url = format!(
        "https://rusefi.com/online/ini/rusefi/{}/{}/{}/{}/{}/{}",
        parsed.branch,
        parsed.year,
        parsed.month,
        parsed.day,
        parsed.bundle_target,
        file_name,
    );
    Some((url, file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proteus_signature() {
        let sig = "rusEFI master.2025.09.02.proteus_f7.4139280449";
        let parsed = parse_rusefi_signature(sig).unwrap();
        assert_eq!(parsed.branch, "master");
        assert_eq!(parsed.bundle_target, "proteus_f7");
        assert_eq!(parsed.hash, "4139280449");
    }

    #[test]
    fn build_download_url() {
        let sig = "rusEFI master.2025.09.02.proteus_f7.4139280449";
        let (url, name) = ini_download_target(sig).unwrap();
        assert_eq!(name, "4139280449.ini");
        assert!(url.contains("proteus_f7/4139280449.ini"));
    }
}
