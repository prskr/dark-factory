//! The receive-pack wire format the gateway reads and writes (design D4): the ref commands
//! of a push request and the `report-status` response that rejects a push.

use gix_packetline::{PacketLineRef, decode};

use super::refs::{InvalidObjectId, ObjectId, RefCommand, RefVerdict};

/// First bytes of a pack file.
const PACK_SIGNATURE: &[u8] = b"PACK";
/// The flush pkt-line that ends a section.
const FLUSH: &[u8] = b"0000";
/// Largest payload of a `side-band-64k` pkt-line: 65516 data bytes minus the band byte.
const MAX_BAND_PAYLOAD: usize = 65515;
/// Side-band channel of the payload data.
const DATA_BAND: u8 = 1;
/// Side-band channel of progress messages (e.g. hook output).
const PROGRESS_BAND: u8 = 2;

/// The command section of a `git-receive-pack` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivePackRequest {
    pub commands: Vec<RefCommand>,
    /// Capabilities the client sent after its first command, e.g. `report-status`.
    pub capabilities: Vec<String>,
    /// Offset of the pack in the body; `None` if the push only deletes refs.
    pub pack_offset: Option<usize>,
}

/// A request body that is not a valid receive-pack command section.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("request ends inside the command section")]
    Truncated,
    #[error("invalid pkt-line: {0}")]
    InvalidPktLine(String),
    #[error("malformed ref command")]
    MalformedCommand,
    #[error("invalid object id in ref command: {0}")]
    InvalidObjectId(#[from] InvalidObjectId),
    #[error("push request contains no ref command")]
    NoCommands,
    #[error("data after the command section is not a pack")]
    UnexpectedData,
}

/// Parses the ref commands, the capabilities and the pack offset of a receive-pack request.
///
/// The commands are small and precede the pack, so the caller passes the start of the
/// (spooled) body and the length of the whole body. If the commands end exactly where
/// `body` ends but the whole body is longer, the rest is the pack; the quarantine checks
/// its signature. A `push-options` section (sent if the client requested the capability)
/// is skipped.
pub fn parse_receive_pack_request(
    body: &[u8],
    body_len: u64,
) -> Result<ReceivePackRequest, ProtocolError> {
    let (lines, mut offset) = read_section(body, 0)?;
    let mut lines = lines.into_iter();
    let first = lines.next().ok_or(ProtocolError::NoCommands)?;
    let mut parts = first.splitn(2, |&b| b == 0);
    let first_command = parts.next().unwrap_or_default();
    let capabilities = parse_capabilities(parts.next().unwrap_or_default())?;

    let mut commands = vec![parse_command(first_command)?];
    for line in lines {
        commands.push(parse_command(line)?);
    }

    if capabilities.iter().any(|c| c == "push-options") {
        (_, offset) = read_section(body, offset)?;
    }

    let rest = body.get(offset..).unwrap_or_default();
    let pack_offset = if rest.is_empty() {
        (u64::try_from(offset).unwrap_or(u64::MAX) < body_len).then_some(offset)
    } else if rest.starts_with(PACK_SIGNATURE) {
        Some(offset)
    } else {
        return Err(ProtocolError::UnexpectedData);
    };
    Ok(ReceivePackRequest {
        commands,
        capabilities,
        pack_offset,
    })
}

/// The `report-status` response with one `ok`/`ng` line per verdict.
///
/// The format is also valid for `report-status-v2` (which only adds optional `option` lines).
/// With `side_band` the report is wrapped into band-1 pkt-lines of `side-band-64k`.
/// Ref names come from parsed pkt-lines, so every report line fits into one pkt-line.
pub fn report_status(verdicts: &[RefVerdict], side_band: bool) -> Vec<u8> {
    let mut report = Vec::new();
    push_pkt_line(&mut report, b"unpack ok\n");
    for verdict in verdicts {
        let line = match verdict.reason {
            None => format!("ok {}\n", verdict.name),
            Some(reason) => format!("ng {} {reason}\n", verdict.name),
        };
        push_pkt_line(&mut report, line.as_bytes());
    }
    report.extend_from_slice(FLUSH);
    if !side_band {
        return report;
    }

    let mut wrapped = Vec::with_capacity(report.len() + 16);
    for chunk in report.chunks(MAX_BAND_PAYLOAD) {
        let mut data = Vec::with_capacity(chunk.len() + 1);
        data.push(DATA_BAND);
        data.extend_from_slice(chunk);
        push_pkt_line(&mut wrapped, &data);
    }
    wrapped.extend_from_slice(FLUSH);
    wrapped
}

/// Whether a forge's `report-status` answer says every ref was updated: `unpack ok`, no `ng`
/// line, and a complete report. With `side_band` the report is read from band 1; a fatal error
/// (band 3) fails it.
pub fn forge_report_ok(body: &[u8], side_band: bool) -> bool {
    let demuxed;
    let report = if side_band {
        let Ok((lines, _)) = read_section(body, 0) else {
            return false;
        };
        let mut data = Vec::new();
        for line in lines {
            match line.split_first() {
                Some((&DATA_BAND, payload)) => data.extend_from_slice(payload),
                Some((&PROGRESS_BAND, _)) => {}
                _ => return false,
            }
        }
        demuxed = data;
        demuxed.as_slice()
    } else {
        body
    };
    let Ok((lines, _)) = read_section(report, 0) else {
        return false;
    };
    let mut lines = lines.into_iter().map(strip_newline);
    lines.next() == Some(b"unpack ok".as_slice()) && lines.all(|line| !line.starts_with(b"ng "))
}

/// Whether the client requested `side-band-64k`.
///
/// Plain `side-band` (1000-byte packets) is not supported: Git clients prefer
/// `side-band-64k`, which the supported forges advertise.
pub fn side_band_requested(capabilities: &[String]) -> bool {
    capabilities.iter().any(|c| c == "side-band-64k")
}

/// Whether the client requested a status report (`report-status` or `report-status-v2`).
///
/// Without one the client reads no report, and a rejection can only be an empty response.
pub fn report_status_requested(capabilities: &[String]) -> bool {
    capabilities
        .iter()
        .any(|c| c == "report-status" || c == "report-status-v2")
}

/// Reads the data pkt-lines from `start` up to the next flush; returns them and the offset
/// after the flush.
fn read_section(body: &[u8], start: usize) -> Result<(Vec<&[u8]>, usize), ProtocolError> {
    let mut lines = Vec::new();
    let mut offset = start;
    loop {
        let rest = body.get(offset..).ok_or(ProtocolError::Truncated)?;
        let decoded = decode::streaming(rest)
            .map_err(|err| ProtocolError::InvalidPktLine(err.to_string()))?;
        let decode::Stream::Complete {
            line,
            bytes_consumed,
        } = decoded
        else {
            return Err(ProtocolError::Truncated);
        };
        offset += bytes_consumed;
        match line {
            PacketLineRef::Data(data) => lines.push(data),
            PacketLineRef::Flush => return Ok((lines, offset)),
            PacketLineRef::Delimiter | PacketLineRef::ResponseEnd => {
                return Err(ProtocolError::InvalidPktLine(
                    "unexpected special packet".to_owned(),
                ));
            }
        }
    }
}

/// Parses `<old> <new> <ref>` with an optional trailing newline.
fn parse_command(line: &[u8]) -> Result<RefCommand, ProtocolError> {
    let line = std::str::from_utf8(strip_newline(line))
        .map_err(|_utf8| ProtocolError::MalformedCommand)?;
    let mut fields = line.splitn(3, ' ');
    let (Some(old), Some(new), Some(name)) = (fields.next(), fields.next(), fields.next()) else {
        return Err(ProtocolError::MalformedCommand);
    };
    if name.is_empty() || name.contains(' ') {
        return Err(ProtocolError::MalformedCommand);
    }
    Ok(RefCommand {
        old: ObjectId::parse(old)?,
        new: ObjectId::parse(new)?,
        name: name.to_owned(),
    })
}

/// Parses the space-separated capability list after the NUL of the first command.
fn parse_capabilities(raw: &[u8]) -> Result<Vec<String>, ProtocolError> {
    let raw =
        std::str::from_utf8(strip_newline(raw)).map_err(|_utf8| ProtocolError::MalformedCommand)?;
    Ok(raw
        .split(' ')
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect())
}

fn strip_newline(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

/// Appends `data` as one pkt-line; `data` is at most 65516 bytes.
fn push_pkt_line(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(format!("{:04x}", data.len() + 4).as_bytes());
    out.extend_from_slice(data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::refs::Reason;

    const ZERO: &str = "0000000000000000000000000000000000000000";
    const OLD: &str = "1111111111111111111111111111111111111111";
    const NEW: &str = "2222222222222222222222222222222222222222";

    fn pkt(data: &str) -> Vec<u8> {
        let mut out = Vec::new();
        push_pkt_line(&mut out, data.as_bytes());
        out
    }

    fn body(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    fn parse(body: &[u8]) -> Result<ReceivePackRequest, ProtocolError> {
        parse_receive_pack_request(body, u64::try_from(body.len()).unwrap())
    }

    #[test]
    fn commands_ending_at_the_head_boundary_leave_the_rest_as_pack() {
        let first = format!("{OLD} {NEW} refs/heads/factory/wip/DF-7\0 report-status");
        let head = body(&[&pkt(&first), FLUSH]);
        let whole = u64::try_from(head.len()).unwrap() + 100;

        let parsed = parse_receive_pack_request(&head, whole).unwrap();

        assert_eq!(parsed.pack_offset, Some(head.len()));
    }

    fn command(old: &str, new: &str, name: &str) -> RefCommand {
        RefCommand {
            old: ObjectId::parse(old).unwrap(),
            new: ObjectId::parse(new).unwrap(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn parses_commands_capabilities_and_pack_offset() {
        let first = format!(
            "{OLD} {NEW} refs/heads/factory/task/DF-7\0 report-status side-band-64k quiet\n"
        );
        let second = format!("{ZERO} {NEW} refs/heads/factory/wip/DF-7\n");
        let commands = body(&[&pkt(&first), &pkt(&second), FLUSH]);
        let request = body(&[&commands, b"PACK\0\0\0\x02rest"]);

        let parsed = parse(&request).unwrap();

        assert_eq!(
            parsed.commands,
            vec![
                command(OLD, NEW, "refs/heads/factory/task/DF-7"),
                command(ZERO, NEW, "refs/heads/factory/wip/DF-7"),
            ]
        );
        assert_eq!(
            parsed.capabilities,
            vec!["report-status", "side-band-64k", "quiet"]
        );
        assert_eq!(parsed.pack_offset, Some(commands.len()));
    }

    #[test]
    fn delete_only_push_has_no_pack() {
        let first = format!("{OLD} {ZERO} refs/heads/factory/wip/DF-7\0report-status");
        let request = body(&[&pkt(&first), FLUSH]);

        let parsed = parse(&request).unwrap();

        assert_eq!(
            parsed.commands,
            vec![command(OLD, ZERO, "refs/heads/factory/wip/DF-7")]
        );
        assert_eq!(parsed.capabilities, vec!["report-status"]);
        assert_eq!(parsed.pack_offset, None);
    }

    #[test]
    fn push_options_section_is_skipped() {
        let first = format!("{OLD} {NEW} refs/heads/factory/wip/DF-7\0 report-status push-options");
        let head = body(&[&pkt(&first), FLUSH, &pkt("ci.skip\n"), FLUSH]);
        let request = body(&[&head, b"PACK"]);

        let parsed = parse(&request).unwrap();

        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.pack_offset, Some(head.len()));
    }

    #[test]
    fn input_ending_before_the_flush_is_truncated() {
        let first = pkt(&format!("{OLD} {NEW} refs/heads/main\0report-status"));

        assert_eq!(parse(&first), Err(ProtocolError::Truncated));
        assert_eq!(
            parse(first.get(..20).unwrap()),
            Err(ProtocolError::Truncated)
        );
        assert_eq!(parse(b""), Err(ProtocolError::Truncated));
    }

    #[test]
    fn malformed_requests_are_rejected() {
        let two_fields = body(&[&pkt(&format!("{OLD} {NEW}\0report-status")), FLUSH]);
        assert_eq!(parse(&two_fields), Err(ProtocolError::MalformedCommand));

        let bad_id = body(&[&pkt(&format!("{OLD} xyz refs/heads/main")), FLUSH]);
        assert_eq!(
            parse(&bad_id),
            Err(ProtocolError::InvalidObjectId(InvalidObjectId))
        );

        assert_eq!(parse(FLUSH), Err(ProtocolError::NoCommands));

        let trailing = body(&[
            &pkt(&format!("{OLD} {NEW} refs/heads/main")),
            FLUSH,
            b"junk",
        ]);
        assert_eq!(parse(&trailing), Err(ProtocolError::UnexpectedData));

        assert!(matches!(
            parse(b"zzzz"),
            Err(ProtocolError::InvalidPktLine(_))
        ));
    }

    fn verdicts() -> Vec<RefVerdict> {
        vec![
            RefVerdict {
                name: "refs/heads/factory/wip/DF-7".to_owned(),
                reason: None,
            },
            RefVerdict {
                name: "refs/heads/main".to_owned(),
                reason: Some(Reason::RefNotAllowed),
            },
        ]
    }

    const PLAIN_REPORT: &str = concat!(
        "000eunpack ok\n",
        "0023ok refs/heads/factory/wip/DF-7\n",
        "002ang refs/heads/main df:ref-not-allowed\n",
        "0000",
    );

    #[test]
    fn plain_report_lists_one_line_per_ref() {
        assert_eq!(report_status(&verdicts(), false), PLAIN_REPORT.as_bytes());
    }

    #[test]
    fn side_band_report_wraps_the_report_into_band_one() {
        let expected = body(&[b"0064\x01", PLAIN_REPORT.as_bytes(), FLUSH]);

        assert_eq!(report_status(&verdicts(), true), expected);
    }

    #[test]
    fn side_band_report_splits_into_max_size_packets() {
        let many: Vec<RefVerdict> = (0..3000)
            .map(|i| RefVerdict {
                name: format!("refs/heads/factory/wip/DF-{i}"),
                reason: Some(Reason::OtherRefRejected),
            })
            .collect();
        let plain = report_status(&many, false);

        let wrapped = report_status(&many, true);

        assert!(plain.len() > MAX_BAND_PAYLOAD);
        assert_eq!(wrapped.get(..5).unwrap(), b"fff0\x01");
        let first_payload = wrapped.get(5..5 + MAX_BAND_PAYLOAD).unwrap();
        assert_eq!(first_payload, plain.get(..MAX_BAND_PAYLOAD).unwrap());
        assert!(wrapped.ends_with(b"00000000"));
    }

    #[test]
    fn a_forge_report_is_ok_only_if_unpacked_and_every_ref_updated() {
        let ok = body(&[&pkt("unpack ok\n"), &pkt("ok refs/heads/a\n"), FLUSH]);
        let ng = body(&[
            &pkt("unpack ok\n"),
            &pkt("ok refs/heads/a\n"),
            &pkt("ng refs/heads/b pre-receive hook declined\n"),
            FLUSH,
        ]);
        let unpack_failed = body(&[&pkt("unpack index-pack failed\n"), FLUSH]);
        let truncated = pkt("unpack ok\n");

        assert!(forge_report_ok(&ok, false));
        assert!(!forge_report_ok(&ng, false));
        assert!(!forge_report_ok(&unpack_failed, false));
        assert!(!forge_report_ok(&truncated, false));
        assert!(!forge_report_ok(b"", false));
    }

    #[test]
    fn a_side_band_forge_report_is_read_from_band_one() {
        let band = |n: u8, data: &[u8]| {
            let mut line = vec![n];
            line.extend_from_slice(data);
            let mut out = Vec::new();
            push_pkt_line(&mut out, &line);
            out
        };
        let report = body(&[&pkt("unpack ok\n"), &pkt("ok refs/heads/a\n"), FLUSH]);
        let (first, second) = report.split_at(7);
        let ok = body(&[
            &band(2, b"remote: hook output\n"),
            &band(1, first),
            &band(1, second),
            FLUSH,
        ]);
        let fatal = body(&[&band(3, b"fatal\n"), FLUSH]);
        let ng_report = body(&[
            &pkt("unpack ok\n"),
            &pkt("ng refs/heads/a declined\n"),
            FLUSH,
        ]);
        let ng = body(&[&band(1, &ng_report), FLUSH]);

        assert!(forge_report_ok(&ok, true));
        assert!(!forge_report_ok(&fatal, true));
        assert!(!forge_report_ok(&ng, true));
        assert!(forge_report_ok(
            &report_status(&verdicts()[..1], true),
            true
        ));
    }

    #[test]
    fn capability_helpers_detect_requested_features() {
        let caps = |list: &[&str]| list.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>();

        assert!(side_band_requested(&caps(&[
            "report-status",
            "side-band-64k"
        ])));
        assert!(!side_band_requested(&caps(&["side-band"])));
        assert!(report_status_requested(&caps(&["report-status"])));
        assert!(report_status_requested(&caps(&["report-status-v2"])));
        assert!(!report_status_requested(&caps(&["quiet"])));
    }
}
