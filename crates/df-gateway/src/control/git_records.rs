//! `GatewayGit`: the control plane side of push records (iter2-git-proxy design D6).

use std::time::Duration;

use df_core::RunId;
use df_db::git_pushes::{self, PushRecord, PushVerdict, RecordOutcome, RefUpdate};
use df_db::{DbError, PgPool};
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_git_server::GatewayGit;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

/// Age after which a `forwarding` record whose final call never came is marked `unknown`.
pub const STALE_FORWARDING: Duration = Duration::from_mins(10);

/// How often the timeout scan runs.
pub const SCAN_INTERVAL: Duration = Duration::from_secs(60);

/// Serves `RecordPush` from the database.
pub struct GitRecords {
    pool: PgPool,
}

impl GitRecords {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl GatewayGit for GitRecords {
    async fn record_push(
        &self,
        request: Request<pb::PushRecord>,
    ) -> Result<Response<pb::RecordPushResponse>, Status> {
        let record = parse(request.into_inner())?;
        match git_pushes::record_push(&self.pool, &record).await {
            Ok(outcome) => {
                if outcome == RecordOutcome::Unchanged {
                    info!(push_id = %record.push_id, verdict = record.verdict.as_str(), "push record unchanged");
                }
                Ok(Response::new(pb::RecordPushResponse {}))
            }
            Err(DbError::NotFound(what)) => Err(Status::not_found(what)),
            Err(DbError::InvalidData(what)) => Err(Status::invalid_argument(what)),
            Err(error) => {
                warn!(%error, push_id = %record.push_id, "storing a push record failed");
                Err(Status::unavailable("push record not stored"))
            }
        }
    }
}

/// Marks stale `forwarding` records as `unknown` every [`SCAN_INTERVAL`] until `stop`.
pub async fn scan_stale_pushes(pool: PgPool, stop: CancellationToken) {
    let mut interval = tokio::time::interval(SCAN_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            _ = interval.tick() => {}
        }
        match git_pushes::mark_stale_forwarding(&pool, STALE_FORWARDING).await {
            Ok(0) => {}
            Ok(marked) => info!(marked, "marked stale forwarding pushes as unknown"),
            Err(error) => warn!(%error, "scanning stale push records failed"),
        }
    }
}

fn parse(record: pb::PushRecord) -> Result<PushRecord, Status> {
    let push_id = Uuid::parse_str(&record.push_id)
        .map_err(|error| Status::invalid_argument(format!("push_id: {error}")))?;
    let run_id = Uuid::parse_str(&record.run_id)
        .map(RunId::from_uuid)
        .map_err(|error| Status::invalid_argument(format!("run_id: {error}")))?;
    let verdict = verdict(record.verdict())?;
    let refs = record
        .refs
        .into_iter()
        .map(|r| {
            Ok(RefUpdate {
                verdict: self::verdict(r.verdict())?,
                name: r.name,
                old_sha: r.old_sha,
                new_sha: r.new_sha,
                reason: r.reason,
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;
    // A push rejected before its commands were parsed has no refs.
    if refs.is_empty() && verdict != PushVerdict::Rejected {
        return Err(Status::invalid_argument(
            "refs: a push that was not rejected has at least one ref update",
        ));
    }
    let duration = record
        .duration
        .map(Duration::try_from)
        .transpose()
        .map_err(|error| Status::invalid_argument(format!("duration: {error}")))?
        .unwrap_or_default();
    Ok(PushRecord {
        push_id,
        run_id,
        gateway_pod: record.gateway_pod,
        refs,
        verdict,
        bytes: record.bytes,
        duration,
    })
}

fn verdict(verdict: pb::PushVerdict) -> Result<PushVerdict, Status> {
    match verdict {
        pb::PushVerdict::Unspecified => Err(Status::invalid_argument("verdict is unspecified")),
        pb::PushVerdict::Forwarding => Ok(PushVerdict::Forwarding),
        pb::PushVerdict::Accepted => Ok(PushVerdict::Accepted),
        pb::PushVerdict::Rejected => Ok(PushVerdict::Rejected),
        pb::PushVerdict::UpstreamFailed => Ok(PushVerdict::UpstreamFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> pb::PushRecord {
        pb::PushRecord {
            push_id: Uuid::nil().to_string(),
            run_id: Uuid::nil().to_string(),
            gateway_pod: "gateway-0".into(),
            refs: vec![pb::RefUpdate {
                name: "refs/heads/factory/task/DF-1".into(),
                old_sha: "0".repeat(40),
                new_sha: "a".repeat(40),
                verdict: pb::PushVerdict::Accepted.into(),
                reason: String::new(),
            }],
            verdict: pb::PushVerdict::Accepted.into(),
            bytes: 7,
            duration: Some(crate::flow::duration_proto(Duration::from_secs(1))),
        }
    }

    #[test]
    fn a_valid_record_parses() {
        let record = parse(valid()).unwrap();
        assert_eq!(record.verdict, PushVerdict::Accepted);
        assert_eq!(record.duration, Duration::from_secs(1));
    }

    #[test]
    fn a_rejected_record_without_refs_parses() {
        let mut message = valid();
        message.verdict = pb::PushVerdict::Rejected.into();
        message.refs.clear();
        let record = parse(message).unwrap();
        assert_eq!(record.verdict, PushVerdict::Rejected);
        assert!(record.refs.is_empty());
    }

    #[test]
    fn invalid_records_are_invalid_arguments() {
        let cases: [fn(&mut pb::PushRecord); 4] = [
            |r| r.push_id = "nope".into(),
            |r| r.run_id = String::new(),
            |r| r.verdict = pb::PushVerdict::Unspecified.into(),
            |r| r.refs.clear(),
        ];
        for change in cases {
            let mut record = valid();
            change(&mut record);
            let status = parse(record).unwrap_err();
            assert_eq!(status.code(), tonic::Code::InvalidArgument);
        }
    }
}
