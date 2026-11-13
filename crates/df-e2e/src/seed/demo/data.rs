//! What the demo profile writes: personas, projects and the shape of a demo task. The tasks
//! themselves are in `tasks.rs`.

use df_core::{RunOutcome, TaskStatus};

pub(super) const ANTHROPIC: &str = "Anthropic";
pub(super) const OPENAI: &str = "OpenAI";
pub(super) const SONNET: &str = "claude-sonnet-4-5";
pub(super) const OPUS: &str = "claude-opus-4-1";
pub(super) const GPT: &str = "gpt-5-mini";

pub(super) const MINUTE: i64 = 60;
pub(super) const HOUR: i64 = 60 * MINUTE;
pub(super) const DAY: i64 = 24 * HOUR;

pub(super) struct DemoPersona {
    pub name: &'static str,
    pub description: &'static str,
    pub system_prompt: &'static str,
    pub harness: &'static str,
    pub model: &'static str,
    pub provider: &'static str,
}

pub(super) const PERSONAS: [DemoPersona; 3] = [
    DemoPersona {
        name: "Linus",
        description: "Engineer. Implements features and fixes in small commits, writes the test first and keeps pull requests easy to review.",
        system_prompt: "You are Linus, a careful software engineer. Work test-first, keep commits small and explain trade-offs in the pull request description.",
        harness: df_core::config::CLAUDE_CODE_HARNESS,
        model: SONNET,
        provider: ANTHROPIC,
    },
    DemoPersona {
        name: "Grace",
        description: "Reviewer. Reads changes critically, checks edge cases, security and documentation, and reports findings with file and line.",
        system_prompt: "You are Grace, a code reviewer. Check the change against the task, name the file and line of every finding and separate blocking issues from suggestions.",
        harness: df_core::config::CLAUDE_CODE_HARNESS,
        model: OPUS,
        provider: ANTHROPIC,
    },
    DemoPersona {
        name: "Ada",
        description: "Researcher. Investigates problems, measures before and after, and hands over findings as a report with data.",
        system_prompt: "You are Ada, a researcher. Measure before you conclude, state how sure you are and hand over a short report with the raw data attached.",
        harness: df_core::config::OPENCODE_HARNESS,
        model: GPT,
        provider: OPENAI,
    },
];

pub(super) struct DemoProject {
    pub key: &'static str,
    pub name: &'static str,
    pub repo_url: &'static str,
    pub issue_tracker_url: Option<&'static str>,
    pub image: &'static str,
    pub hidden: &'static [&'static str],
}

pub(super) const PROJECTS: [DemoProject; 3] = [
    DemoProject {
        key: "WEB",
        name: "Storefront",
        repo_url: "https://git.acme.example/shop/storefront",
        issue_tracker_url: Some("https://git.acme.example/shop/storefront/issues"),
        image: "ghcr.io/acme/dev-images/node:22",
        hidden: &[],
    },
    DemoProject {
        key: "PAY",
        name: "Payments API",
        repo_url: "https://git.acme.example/platform/payments-api",
        issue_tracker_url: None,
        image: "ghcr.io/acme/dev-images/rust:1.90",
        hidden: &[],
    },
    DemoProject {
        key: "OPS",
        name: "Homelab infrastructure",
        repo_url: "https://git.acme.example/home/infrastructure",
        issue_tracker_url: None,
        image: "ghcr.io/acme/dev-images/ops-tools:2026.10",
        hidden: &["talos/**"],
    },
];

/// The web address of pull request `number` of the project `key`.
pub(super) fn pr_url(key: &str, number: i64) -> Option<String> {
    let project = PROJECTS.iter().find(|p| p.key == key)?;
    Some(format!("{}/pulls/{number}", project.repo_url))
}

/// Who created a task.
#[derive(Clone, Copy)]
pub(super) enum Creator {
    /// A board member, by display name.
    Human(&'static str),
    /// The agent of the parent task.
    Agent,
}

/// A pull request of a task, opened by the factory or outside it.
#[derive(Clone, Copy)]
pub(super) struct Pr {
    pub number: i64,
    pub title: &'static str,
    pub body: &'static str,
    /// The branch the task is attached to; the pull request was opened outside the factory.
    pub attached_branch: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub(super) enum ToolKind {
    Read,
    Edit,
    Execute,
    Search,
}

/// One step of a run's transcript after the assignment.
#[derive(Clone, Copy)]
pub(super) enum Step {
    /// A message of the agent.
    Agent(&'static str),
    /// A finished tool call.
    Tool {
        title: &'static str,
        kind: ToolKind,
        input: &'static str,
        output: &'static str,
    },
    /// A follow-up message of a board member, delivered with this run.
    Message {
        author: &'static str,
        text: &'static str,
    },
}

/// Token usage of one model within a run, spread over `requests` requests.
#[derive(Clone, Copy)]
pub(super) struct Usage {
    pub model: &'static str,
    pub requests: i64,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

pub(super) struct DemoRun {
    /// Seconds before the seeding that the run started.
    pub start_ago: i64,
    pub minutes: i64,
    /// How the run ends.
    pub outcome: RunOutcome,
    pub detail: &'static str,
    /// The summary of `complete_task`.
    pub summary: &'static str,
    /// When the provider allows the next request, seconds after the seeding (rate limit).
    pub retry_in: Option<i64>,
    pub steps: Vec<Step>,
    pub usage: Vec<Usage>,
}

impl DemoRun {
    pub(super) fn new(start_ago: i64, minutes: i64, outcome: RunOutcome) -> Self {
        Self {
            start_ago,
            minutes,
            outcome,
            detail: "",
            summary: "",
            retry_in: None,
            steps: Vec::new(),
            usage: Vec::new(),
        }
    }

    pub(super) fn detail(mut self, detail: &'static str) -> Self {
        self.detail = detail;
        self
    }

    pub(super) fn summary(mut self, summary: &'static str) -> Self {
        self.summary = summary;
        self
    }

    pub(super) fn retry_in(mut self, seconds: i64) -> Self {
        self.retry_in = Some(seconds);
        self
    }

    pub(super) fn steps(mut self, steps: Vec<Step>) -> Self {
        self.steps = steps;
        self
    }

    pub(super) fn usage(mut self, usage: Vec<Usage>) -> Self {
        self.usage = usage;
        self
    }
}

pub(super) struct Question {
    pub text: &'static str,
    pub suggested_answers: &'static [&'static str],
}

#[derive(Clone, Copy)]
pub(super) enum Content {
    Text(&'static str),
    Image(&'static [u8]),
}

pub(super) struct Artifact {
    pub name: &'static str,
    pub content: Content,
}

pub(super) struct DemoTask {
    pub project: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub persona: &'static str,
    pub status: TaskStatus,
    pub status_line: &'static str,
    pub created_by: Creator,
    /// Seconds before the seeding that the task was created.
    pub created_ago: i64,
    pub issue: Option<i64>,
    /// Title of the parent task of the same project.
    pub parent: Option<&'static str>,
    /// Titles of tasks of the same project this one waits for to complete.
    pub depends_on: Vec<&'static str>,
    pub pr: Option<Pr>,
    /// Seconds after the seeding that the scheduler may start the task again (`queued`,
    /// `rate_limited`).
    pub resume_in: Option<i64>,
    /// Consecutive automatic resumes after infrastructure failures.
    pub auto_resumes: i32,
    /// Seconds before the seeding that a board member marked the task done or its pull request
    /// was merged.
    pub done_ago: Option<i64>,
    pub runs: Vec<DemoRun>,
    pub question: Option<Question>,
    /// Written by the last run.
    pub artifacts: Vec<Artifact>,
}

impl DemoTask {
    pub(super) fn new(
        project: &'static str,
        title: &'static str,
        persona: &'static str,
        status: TaskStatus,
        created_by: Creator,
        created_ago: i64,
    ) -> Self {
        Self {
            project,
            title,
            description: "",
            persona,
            status,
            status_line: "",
            created_by,
            created_ago,
            issue: None,
            parent: None,
            depends_on: Vec::new(),
            pr: None,
            resume_in: None,
            auto_resumes: 0,
            done_ago: None,
            runs: Vec::new(),
            question: None,
            artifacts: Vec::new(),
        }
    }
}
