//! A completed oversized record must not disappear before the budget check.
use iptv_core::import_limits::{ImportLimits, LimitKind};
use iptv_epg::{DecodeError, ParseOptions, XmltvParser};
#[test]
fn record_budget_is_enforced_before_emitting_complete_or_split_records() {
    let text = format!(
        "<tv><channel id=\"s\"><display-name>{}</display-name></channel></tv>",
        "x".repeat(1024)
    );
    for chunk in [1, 97, text.len()] {
        let mut parser = XmltvParser::new(ParseOptions {
            limits: ImportLimits {
                record_bytes: 128,
                ..Default::default()
            },
            ..Default::default()
        });
        let mut emitted = 0;
        let mut result = Ok(());
        for bytes in text.as_bytes().chunks(chunk) {
            result = parser.feed(bytes, &mut |_| emitted += 1);
            if result.is_err() {
                break;
            }
        }
        assert!(
            matches!(result,Err(DecodeError::Limit(error)) if error.kind==LimitKind::RecordBytes)
        );
        assert_eq!(emitted, 0);
    }
}
