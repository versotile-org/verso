use std::collections::HashMap;

/// Disposition type.
#[derive(Debug, PartialEq, Eq)]
pub enum DispositionType {
    Attachment,
    Inline,
    FormData,
}

/// Content disposition.
pub struct ContentDisposition {
    pub disposition: DispositionType,
    pub params: HashMap<String, String>,
}

impl ContentDisposition {
    /// Get filename from params. Returns `filename*` if exists, otherwise returns `filename`.
    /// This function will replace `/` into `-` in the filename.
    pub fn filename(&self) -> Option<String> {
        self.params
            .get("filename*")
            .or_else(|| self.params.get("filename"))
            .map(|filename| filename.replace('/', "-"))
    }
}

/// Parse `Content-Disposition` header.
pub fn parse_content_disposition(header: &str) -> Option<ContentDisposition> {
    let (disposition, params_str) = match header.split_once(';') {
        Some((type_part, params)) => (type_part, params),
        None => (header, ""),
    };

    let disposition = parse_disposition_type(disposition)?;
    let params = parse_params(params_str.split(';').collect());

    Some(ContentDisposition {
        disposition,
        params,
    })
}

fn parse_disposition_type(disposition: &str) -> Option<DispositionType> {
    match &disposition.trim().to_lowercase()[..] {
        "inline" => Some(DispositionType::Inline),
        "attachment" => Some(DispositionType::Attachment),
        "form-data" => Some(DispositionType::FormData),
        _ => None,
    }
}

fn parse_params(params_parts: Vec<&str>) -> HashMap<String, String> {
    let mut params: HashMap<String, String> = HashMap::new();

    for part in params_parts {
        let Some((key, value)) = part.split_once('=').map(|(k, v)| (k.trim(), v.trim())) else {
            continue;
        };

        // if `"` exists on both side of value, it is a quoted-string, trim `"`
        let value = if value.starts_with('"') && value.ends_with('"') {
            value.trim_matches('"')
        } else {
            value
        };

        // If key has `*` at the end which means its value is RFC5987 encoded.
        // We only handle `utf-8` encoding for now.
        let value = if key.ends_with('*') && value.to_lowercase().starts_with("utf-8''") {
            // remove `utf-8''` from the value
            let value = value.split_at(7).1;
            &percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .to_string()
        } else {
            value
        };

        params.insert(key.to_string(), value.to_string());
    }

    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_disposition() {
        let content_disposition = parse_content_disposition("");
        assert!(content_disposition.is_none());

        let content_disposition = parse_content_disposition("unknown");
        assert!(content_disposition.is_none());

        let content_disposition = parse_content_disposition("inline");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Inline);
        assert_eq!(content_disposition.params.get("filename"), None);

        let content_disposition = parse_content_disposition(" attachment; name=\"field_name\"");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Attachment);
        assert_eq!(
            content_disposition.params.get("name"),
            Some(&"field_name".to_string())
        );
        assert_eq!(content_disposition.params.get("filename"), None);

        let content_disposition =
            parse_content_disposition("form-data; name=field_name; filename=\"te%20st.jpg\"");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::FormData);
        assert_eq!(
            content_disposition.params.get("name"),
            Some(&"field_name".to_string())
        );
        assert_eq!(
            content_disposition.params.get("filename"),
            Some(&"te%20st.jpg".to_string())
        );

        let content_disposition = parse_content_disposition(
            "attachment; name= \"field_name\"; filename*=\"UTF-8''測試.html\"",
        );
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Attachment);
        assert_eq!(
            content_disposition.params.get("name"),
            Some(&"field_name".to_string())
        );
        assert_eq!(
            content_disposition.params.get("filename*"),
            Some(&"測試.html".to_string())
        );

        let content_disposition =
            parse_content_disposition("attachment; filename*=utf-8''file%20name.jpg");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Attachment);
        assert_eq!(content_disposition.params.get("name"), None);
        assert_eq!(
            content_disposition.params.get("filename*"),
            Some(&"file name.jpg".to_string())
        );

        let content_disposition = parse_content_disposition("attachment; filename*=UTF-8''");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Attachment);
        assert_eq!(
            content_disposition.params.get("filename*"),
            Some(&"".to_string())
        );

        let content_disposition =
            parse_content_disposition("attachment; filename*=UTF-8''%E6%B8%AC%E8%A9%A6.html");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Attachment);
        assert_eq!(
            content_disposition.params.get("filename*"),
            Some(&"測試.html".to_string())
        );

        let content_disposition =
            parse_content_disposition("attachment; filename=../path/to/file.html");
        assert!(content_disposition.is_some());
        let content_disposition = content_disposition.unwrap();
        assert_eq!(content_disposition.disposition, DispositionType::Attachment);
        assert_eq!(
            content_disposition.filename(),
            Some("..-path-to-file.html".to_string())
        );
    }
}
