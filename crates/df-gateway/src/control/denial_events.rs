//! `GatewayEvents`: the control plane side of denial reports (iter2-egress-approvals design D2).

use df_core::RunId;
use df_db::PgPool;
use df_db::network_access::{DenialOutcome, raise_from_denial};
use df_db::requests::{Raised, RequestError};
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_events_server::GatewayEvents;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Serves `ReportDenial` from the database.
pub struct DenialEvents {
    pool: PgPool,
}

impl DenialEvents {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl GatewayEvents for DenialEvents {
    async fn report_denial(
        &self,
        request: Request<pb::Denial>,
    ) -> Result<Response<pb::ReportDenialResponse>, Status> {
        let denial = request.into_inner();
        let run_id = Uuid::parse_str(&denial.run_id)
            .map(RunId::from_uuid)
            .map_err(|error| Status::invalid_argument(format!("run_id: {error}")))?;
        let (host, port) = (denial.host.as_str(), denial.port);
        match raise_from_denial(&self.pool, run_id, host, port).await {
            Ok(DenialOutcome::Raised(Raised::Request { request, new: true })) => {
                info!(request_id = %request.id, %run_id, host, port, "network access request raised from a denial");
            }
            Ok(outcome) => debug!(?outcome, %run_id, host, port, "denial report ignored"),
            Err(RequestError::Invalid(what)) => return Err(Status::invalid_argument(what)),
            // The gateway cannot do anything about it; the next denial tries again.
            Err(RequestError::TooManyPending) => {
                info!(%run_id, host, port, "denial report ignored: too many pending requests");
            }
            Err(error) => {
                warn!(%error, %run_id, host, port, "handling a denial report failed");
                return Err(Status::unavailable("denial report not handled"));
            }
        }
        Ok(Response::new(pb::ReportDenialResponse {}))
    }
}
