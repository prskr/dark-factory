//! The demo tasks: every state, a parent with subtasks, a dependency, an attached pull request,
//! a linked issue, a pending question and artifacts. Texts are meant to read like real work.

use df_core::{RunOutcome, TaskStatus};

use super::data::{
    Artifact, Content, Creator, DAY, DemoRun, DemoTask, HOUR, MINUTE, Pr, Question, SONNET, Step,
    ToolKind, Usage,
};

/// The demo image: p95 latency of the product search per release.
const SEARCH_LATENCY_PNG: &[u8] = include_bytes!("../../../assets/demo/search-latency.png");

const SEARCH_REPORT: &str = "\
# Product search is slow on mobile

## Summary

The p95 latency of `/api/search` on mobile is 1.7 s, three times the desktop value. The cause is
the payload: every hit carries the full product description and all image variants.

## Findings

1. A mobile response is 412 KB on average, 380 KB of it descriptions and image URLs the result
   list never shows.
2. The query itself takes 38 ms at p95. The time goes into serialization and the transfer over
   mobile networks.
3. Release 2026.44 moved the description into the search index, which added the payload.

## Recommendation

Return `id`, `name`, `price` and one thumbnail URL from `/api/search` and load the rest on the
product page. A first measurement with that projection: p95 590 ms (see `search-latency.png`).

## Open questions

- The mobile app caches search results for one hour. Whether a smaller payload changes the cache
  hit rate is not measured yet.
";

const PROFILER_LOG: &str = "\
$ node --cpu-prof scripts/replay-search.js --device mobile --queries 500
replaying 500 queries from access-2026-11-09.log
  p50    812 ms
  p95   1708 ms
  p99   2310 ms
top of the profile (self time):
  41.2%  JSON.stringify            src/search/serialize.ts:58
  22.7%  gzipSync                  node:zlib
   9.4%  mapHit                    src/search/serialize.ts:31
   3.1%  elasticsearch.search      src/search/client.ts:77
done in 6m12s
";

const TEST_LOG: &str = "\
$ npm test -- --run src/checkout/AddressForm
 RUN  v2.1.4 /workspace/storefront

 ✓ src/checkout/AddressForm.test.tsx (9 tests) 412ms
   ✓ renders the saved address as the default
   ✓ validates the postal code for DE, AT and CH
   ✓ shows an inline error for a missing street
   ✓ keeps the entered values when the country changes
   ✓ calls onChange with a normalized address
   ✓ is usable with the keyboard only

 Test Files  1 passed (1)
      Tests  9 passed (9)
   Duration  1.31s
";

fn sonnet(requests: i64, input: i64, output: i64, cache_read: i64, cache_write: i64) -> Usage {
    Usage {
        model: SONNET,
        requests,
        input,
        output,
        cache_read,
        cache_write,
    }
}

fn tool(title: &'static str, kind: ToolKind, input: &'static str, output: &'static str) -> Step {
    Step::Tool {
        title,
        kind,
        input,
        output,
    }
}

/// All demo tasks in the order they are created: parents and dependency targets before the tasks
/// that refer to them.
pub(super) fn all() -> Vec<DemoTask> {
    let mut tasks = storefront();
    tasks.extend(payments());
    tasks.extend(homelab());
    tasks
}

#[expect(clippy::too_many_lines, reason = "data of the demo tasks")]
fn storefront() -> Vec<DemoTask> {
    let mara = Creator::Human("Mara Okafor");
    let jonas = Creator::Human("Jonas Weber");
    let checkout = "Checkout redesign";
    vec![
        DemoTask {
            description: "Rework the checkout into a single page with address, shipping and payment sections. Start with the address form, have it reviewed, then move on to shipping and payment. Keep the order API as it is.",
            status_line: "Waiting for the subtasks WEB-2 and WEB-3",
            runs: vec![
                DemoRun::new(3 * DAY, 18, RunOutcome::EndTurn)
                    .steps(vec![
                        tool("Read src/checkout/", ToolKind::Read, r#"{"path":"src/checkout"}"#, "CheckoutPage.tsx, AddressForm.tsx, PaymentStep.tsx, useOrder.ts"),
                        Step::Agent("The checkout is three screens today and the address form is copied into the account page. I created two subtasks: one extracts the address form into a shared component, the other updates the help center screenshots and waits until that change is merged. I continue with shipping and payment when both are done."),
                    ])
                    .usage(vec![sonnet(9, 96_210, 8_940, 512_300, 41_870)]),
            ],
            ..DemoTask::new("WEB", checkout, "Linus", TaskStatus::Waiting, mara, 3 * DAY + 2 * HOUR)
        },
        DemoTask {
            description: "The address form exists twice, in the checkout and on the account page. Extract it into one component with the same validation and use it in both places.",
            status_line: "Pull request #212 is ready for review",
            parent: Some(checkout),
            pr: Some(Pr {
                number: 212,
                title: "Extract AddressForm into a shared component",
                body: "Moves the address form of the checkout into `src/components/AddressForm` and uses it on the account page.\n\n- postal code validation for DE, AT and CH in one place\n- 9 unit tests, keyboard-only use covered\n\nPart of WEB-1.",
                attached_branch: None,
            }),
            runs: vec![
                DemoRun::new(2 * DAY + 20 * HOUR, 41, RunOutcome::Completed)
                    .summary("Extracted the address form into a shared component and used it in the checkout and on the account page. The unit tests pass.")
                    .steps(vec![
                        tool("Search for AddressForm", ToolKind::Search, r#"{"pattern":"AddressForm","path":"src"}"#, "src/checkout/AddressForm.tsx\nsrc/account/AddressForm.tsx (copy)"),
                        tool("Edit src/components/AddressForm.tsx", ToolKind::Edit, r#"{"path":"src/components/AddressForm.tsx"}"#, "created, 118 lines"),
                        tool("npm test -- --run src/checkout/AddressForm", ToolKind::Execute, r#"{"command":"npm test -- --run src/checkout/AddressForm"}"#, "9 passed (9)"),
                        Step::Agent("The two copies had drifted: the account page accepted a postal code of any length. The shared component validates DE, AT and CH like the checkout did, so the account page is stricter now. I mention that in the pull request."),
                    ])
                    .usage(vec![sonnet(23, 211_480, 19_330, 1_402_600, 88_120)]),
            ],
            artifacts: vec![Artifact {
                name: "unit-tests.log",
                content: Content::Text(TEST_LOG),
            }],
            ..DemoTask::new("WEB", "Extract address form component", "Linus", TaskStatus::InReview, Creator::Agent, 2 * DAY + 21 * HOUR)
        },
        DemoTask {
            description: "The help center shows the old account page. Replace its screenshots once the new address form is merged.",
            status_line: "Waiting for WEB-2 to be merged",
            parent: Some(checkout),
            depends_on: vec!["Extract address form component"],
            ..DemoTask::new("WEB", "Update the account page screenshots in the help center", "Linus", TaskStatus::AwaitingDependencies, Creator::Agent, 2 * DAY + 21 * HOUR)
        },
        DemoTask {
            description: "A cart with a 33 % voucher and three items shows a total that is one cent off. Round per line item like the order service does.",
            issue: Some(187),
            status_line: "Pull request #208 was merged",
            pr: Some(Pr {
                number: 208,
                title: "Round discounted line items before summing the cart",
                body: "Fixes #187.\n\nThe cart summed unrounded line totals and rounded once. The order service rounds per line item, so the totals differed by a cent. The cart rounds per line item now, with a test for the 33 % voucher.",
                attached_branch: None,
            }),
            done_ago: Some(2 * DAY),
            runs: vec![
                DemoRun::new(4 * DAY, 22, RunOutcome::Completed)
                    .summary("The cart rounds per line item now, like the order service.")
                    .steps(vec![
                        tool("Read src/cart/total.ts", ToolKind::Read, r#"{"path":"src/cart/total.ts"}"#, "export function cartTotal(items) { ... }"),
                        tool("Edit src/cart/total.ts", ToolKind::Edit, r#"{"path":"src/cart/total.ts"}"#, "1 file changed"),
                        Step::Agent("Reproduced the cent with the voucher from the issue: 3 x 9.99 EUR with 33 % off is 20.08 EUR per line item and 20.08 EUR in the cart only when each line is rounded first."),
                    ])
                    .usage(vec![sonnet(11, 88_950, 7_410, 604_220, 30_500)]),
                DemoRun::new(3 * DAY + 20 * HOUR, 14, RunOutcome::Completed)
                    .summary("Added the test for the 33 % voucher and for a cart with a free item.")
                    .steps(vec![
                        Step::Message {
                            author: "Jonas Weber",
                            text: "Thanks. Please add a test for the 33 % voucher and for a free item, then it is good to merge.",
                        },
                        tool("Edit src/cart/total.test.ts", ToolKind::Edit, r#"{"path":"src/cart/total.test.ts"}"#, "2 tests added"),
                        tool("npm test -- --run src/cart", ToolKind::Execute, r#"{"command":"npm test -- --run src/cart"}"#, "14 passed (14)"),
                    ])
                    .usage(vec![sonnet(8, 61_330, 4_870, 388_040, 12_900)]),
            ],
            ..DemoTask::new("WEB", "Fix rounding of discounted cart totals", "Linus", TaskStatus::Completed, jonas, 4 * DAY + HOUR)
        },
        DemoTask {
            description: "Search on the mobile site feels slow. Find out where the time goes and say what would help. Report with numbers, no code changes.",
            status_line: "Report ready for review",
            runs: vec![
                DemoRun::new(DAY + 5 * HOUR, 52, RunOutcome::Completed)
                    .summary("p95 on mobile is 1.7 s and almost all of it is payload. A thinner response brings it to about 0.6 s. Report, profile and chart are attached.")
                    .steps(vec![
                        tool("node --cpu-prof scripts/replay-search.js", ToolKind::Execute, r#"{"command":"node --cpu-prof scripts/replay-search.js --device mobile --queries 500"}"#, "p50 812 ms, p95 1708 ms, p99 2310 ms"),
                        Step::Agent("The query takes 38 ms. 41 % of the time is JSON.stringify and 23 % gzip, both on a 412 KB response. Release 2026.44 added the full description to every hit."),
                        tool("node scripts/replay-search.js --projection thin", ToolKind::Execute, r#"{"command":"node scripts/replay-search.js --device mobile --projection thin"}"#, "p95 590 ms"),
                    ])
                    .usage(vec![Usage {
                        model: super::data::GPT,
                        requests: 17,
                        input: 148_300,
                        output: 16_020,
                        cache_read: 0,
                        cache_write: 0,
                    }]),
            ],
            artifacts: vec![
                Artifact {
                    name: "search-latency-report.md",
                    content: Content::Text(SEARCH_REPORT),
                },
                Artifact {
                    name: "search-latency.png",
                    content: Content::Image(SEARCH_LATENCY_PNG),
                },
                Artifact {
                    name: "profiler-output.log",
                    content: Content::Text(PROFILER_LOG),
                },
            ],
            ..DemoTask::new("WEB", "Why is product search slow on mobile?", "Ada", TaskStatus::InReview, mara, DAY + 6 * HOUR)
        },
        DemoTask {
            description: "Serve product images as WebP with a JPEG fallback. Convert the existing images in the build step and update the `<picture>` markup.",
            status_line: "The sandbox was lost; the task starts again automatically",
            resume_in: Some(6 * HOUR),
            auto_resumes: 1,
            runs: vec![
                DemoRun::new(5 * HOUR, 7, RunOutcome::SandboxLost)
                    .detail("sandbox is gone: the node was drained")
                    .steps(vec![tool("Read package.json", ToolKind::Read, r#"{"path":"package.json"}"#, "scripts: build, test, lint")])
                    .usage(vec![sonnet(3, 24_100, 1_020, 80_400, 9_800)]),
            ],
            ..DemoTask::new("WEB", "Migrate product images to WebP", "Linus", TaskStatus::Queued, jonas, 6 * HOUR)
        },
    ]
}

fn payments() -> Vec<DemoTask> {
    let mara = Creator::Human("Mara Okafor");
    let jonas = Creator::Human("Jonas Weber");
    vec![
        DemoTask {
            description: "Renovate opened a pull request that bumps serde to 1.x and the build fails. Fix it on the Renovate branch.",
            status_line: "Waiting for CI on #341",
            pr: Some(Pr {
                number: 341,
                title: "chore(deps): update rust crate serde to v1.0.228",
                body: "This PR contains the following updates:\n\n| Package | Change |\n|---|---|\n| serde | `1.0.219` -> `1.0.228` |\n\n---\nThis PR was generated by Renovate.",
                attached_branch: Some("renovate/serde-1.x"),
            }),
            runs: vec![
                DemoRun::new(7 * HOUR, 12, RunOutcome::EndTurn)
                    .steps(vec![
                        tool("cargo build", ToolKind::Execute, r#"{"command":"cargo build --all-targets"}"#, "error[E0277]: the trait bound `Money: Deserialize` is not satisfied"),
                        tool("Edit src/money.rs", ToolKind::Edit, r#"{"path":"src/money.rs"}"#, "derive(Deserialize) added"),
                        Step::Agent("The build passes locally. I pushed the fix to the Renovate branch and wait for the CI result before I report back."),
                    ])
                    .usage(vec![sonnet(10, 71_660, 5_120, 330_900, 18_400)]),
            ],
            ..DemoTask::new("PAY", "Bump serde to 1.x", "Linus", TaskStatus::Waiting, jonas, 7 * HOUR + 10 * MINUTE)
        },
        DemoTask {
            description: "Clients retry POST /v1/charges after timeouts and sometimes charge twice. Accept an `Idempotency-Key` header and return the stored response for a repeated key.",
            status_line: "Waiting for an answer",
            question: Some(Question {
                text: "Should a retry with the same key after 24 hours create a new charge or return the original response?",
                suggested_answers: &[
                    "Return the original response for 24 hours, then accept the key again",
                    "Return the original response for as long as the key is stored",
                    "Reject a reused key with 409 Conflict",
                ],
            }),
            runs: vec![
                DemoRun::new(26 * HOUR, 16, RunOutcome::Blocked)
                    .steps(vec![
                        tool("Read src/api/charges.rs", ToolKind::Read, r#"{"path":"src/api/charges.rs"}"#, "pub async fn create_charge(...)"),
                        Step::Agent("The charge table has no place for a key yet. Before I design the migration I need to know how long a key stays valid, so I asked a question and wait for the answer."),
                    ])
                    .usage(vec![sonnet(7, 52_400, 3_900, 240_100, 14_000)]),
            ],
            ..DemoTask::new("PAY", "Add idempotency keys to POST /v1/charges", "Linus", TaskStatus::Blocked, mara, 27 * HOUR)
        },
        DemoTask {
            description: "A replayed webhook event passes the signature check because the timestamp is not part of the signed payload. Include it and reject events older than five minutes.",
            status_line: "The run failed",
            runs: vec![
                DemoRun::new(2 * DAY, 4, RunOutcome::Failed)
                    .detail("harness exited with status 1: model request failed (upstream status 529, overloaded)")
                    .steps(vec![tool("Read src/webhooks/signature.rs", ToolKind::Read, r#"{"path":"src/webhooks/signature.rs"}"#, "pub fn verify(payload: &[u8], header: &str)")])
                    .usage(vec![sonnet(2, 9_800, 410, 20_000, 6_100)]),
            ],
            ..DemoTask::new("PAY", "Reject replayed webhook events", "Linus", TaskStatus::Failed, mara, 2 * DAY + HOUR)
        },
        DemoTask {
            description: "Write the runbook for a card network outage: how to recognize it, which switches to flip, what to tell merchants and how to recover. Short and usable at 3 am.",
            status_line: "Weekly usage limit reached",
            resume_in: Some(4 * DAY),
            runs: vec![
                DemoRun::new(10 * HOUR, 9, RunOutcome::RateLimited)
                    .detail("Weekly usage limit reached. The limit resets on Monday at 09:00 UTC.")
                    .retry_in(4 * DAY)
                    .steps(vec![tool("Search for outage", ToolKind::Search, r#"{"pattern":"outage","path":"docs"}"#, "docs/incidents/2026-03-network.md")])
                    .usage(vec![Usage {
                        model: super::data::GPT,
                        requests: 6,
                        input: 51_200,
                        output: 2_770,
                        cache_read: 0,
                        cache_write: 0,
                    }]),
            ],
            ..DemoTask::new("PAY", "Write the runbook for card network outages", "Ada", TaskStatus::RateLimited, jonas, 11 * HOUR)
        },
        DemoTask {
            description: "Remove the deprecated /v1/tokens endpoint and its tests. The last client moved to /v2/tokens in October.",
            status_line: "Cancelled by Mara Okafor",
            runs: vec![
                DemoRun::new(5 * DAY, 3, RunOutcome::Cancelled)
                    .detail("stopped")
                    .steps(vec![tool("Search for /v1/tokens", ToolKind::Search, r#"{"pattern":"/v1/tokens"}"#, "3 matches in 2 files")])
                    .usage(vec![sonnet(2, 11_900, 520, 16_200, 4_000)]),
            ],
            ..DemoTask::new("PAY", "Remove the deprecated /v1/tokens endpoint", "Linus", TaskStatus::Cancelled, mara, 5 * DAY + 10 * MINUTE)
        },
    ]
}

fn homelab() -> Vec<DemoTask> {
    let jonas = Creator::Human("Jonas Weber");
    vec![
        DemoTask {
            description: "Add a Grafana dashboard for the node temperatures of the three mini PCs and an alert above 80 degrees. Put it next to the other dashboards in `monitoring/`.",
            status_line: "Pull request #58 is ready for review",
            pr: Some(Pr {
                number: 58,
                title: "Add a node temperature dashboard and alert",
                body: "Adds `monitoring/dashboards/node-temperature.json` and the alert rule `NodeTooHot` (80 degrees for 5 minutes).\n\nThe panels use the existing `node_hwmon_temp_celsius` metric.",
                attached_branch: None,
            }),
            runs: vec![
                DemoRun::new(20 * HOUR, 33, RunOutcome::Completed)
                    .summary("Added the node temperature dashboard and the NodeTooHot alert rule.")
                    .steps(vec![
                        tool("Read monitoring/dashboards/", ToolKind::Read, r#"{"path":"monitoring/dashboards"}"#, "node-overview.json, ups.json"),
                        tool("Edit monitoring/dashboards/node-temperature.json", ToolKind::Edit, r#"{"path":"monitoring/dashboards/node-temperature.json"}"#, "created, 212 lines"),
                        tool("promtool check rules monitoring/alerts.yaml", ToolKind::Execute, r#"{"command":"promtool check rules monitoring/alerts.yaml"}"#, "SUCCESS: 7 rules found"),
                        Step::Agent("The dashboard shows the three nodes side by side with the 80 degree threshold as a red line. The alert uses a five-minute window so a short spike while compiling does not page anyone."),
                    ])
                    .usage(vec![sonnet(19, 160_900, 14_300, 910_700, 52_300)]),
            ],
            ..DemoTask::new("OPS", "Add a Grafana dashboard for node temperatures", "Linus", TaskStatus::InReview, jonas, 21 * HOUR)
        },
        DemoTask {
            description: "The registry holds 140 GB of images and half of them are untagged. Add a weekly cleanup job that keeps the last five tags per image.",
            status_line: "Pull request #52 was merged",
            pr: Some(Pr {
                number: 52,
                title: "Prune old images from the registry weekly",
                body: "Adds a CronJob that runs `regctl` every Sunday at 03:00 and keeps the last five tags per image. Untagged manifests are deleted.\n\nDry run on the current registry: 61 GB would be freed.",
                attached_branch: None,
            }),
            done_ago: Some(5 * DAY),
            runs: vec![
                DemoRun::new(6 * DAY, 27, RunOutcome::Completed)
                    .summary("Added the weekly cleanup CronJob. A dry run frees 61 GB.")
                    .steps(vec![
                        tool("Edit k8s/registry/cleanup-cronjob.yaml", ToolKind::Edit, r#"{"path":"k8s/registry/cleanup-cronjob.yaml"}"#, "created, 41 lines"),
                        tool("kubectl apply --dry-run=server", ToolKind::Execute, r#"{"command":"kubectl apply --dry-run=server -f k8s/registry/cleanup-cronjob.yaml"}"#, "cronjob.batch/registry-cleanup created (server dry run)"),
                    ])
                    .usage(vec![sonnet(12, 97_400, 7_950, 622_800, 21_100)]),
            ],
            ..DemoTask::new("OPS", "Prune old container images from the registry", "Linus", TaskStatus::Completed, jonas, 6 * DAY + HOUR)
        },
        DemoTask {
            description: "Document how to restore the cluster from the nightly backup after losing a node, step by step, with the commands. Mention what is not backed up.",
            status_line: "Waiting for feedback",
            runs: vec![
                DemoRun::new(9 * HOUR, 24, RunOutcome::EndTurn)
                    .steps(vec![
                        tool("Read docs/backup.md", ToolKind::Read, r#"{"path":"docs/backup.md"}"#, "The backup runs nightly at 02:00 ..."),
                        Step::Agent("The nightly backup covers the etcd snapshot and the persistent volumes of the `data` namespace. The Grafana dashboards and the registry are not backed up. I wrote the restore steps and listed both gaps at the end. Tell me if the registry should be added to the backup, then I extend the document."),
                    ])
                    .usage(vec![Usage {
                        model: super::data::GPT,
                        requests: 8,
                        input: 64_800,
                        output: 6_540,
                        cache_read: 0,
                        cache_write: 0,
                    }]),
            ],
            ..DemoTask::new("OPS", "Document the disaster recovery procedure", "Ada", TaskStatus::Waiting, jonas, 10 * HOUR)
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_task_title_is_unique_within_its_project() {
        let tasks = all();
        let mut seen = std::collections::HashSet::new();
        for task in &tasks {
            assert!(seen.insert((task.project, task.title)), "{}", task.title);
        }
    }

    #[test]
    fn parents_and_dependencies_come_before_the_tasks_that_name_them() {
        let tasks = all();
        for (index, task) in tasks.iter().enumerate() {
            for earlier in task.parent.iter().chain(&task.depends_on) {
                assert!(
                    tasks[..index]
                        .iter()
                        .any(|t| t.project == task.project && t.title == *earlier),
                    "{} names {earlier} before it exists",
                    task.title
                );
            }
        }
    }
}
