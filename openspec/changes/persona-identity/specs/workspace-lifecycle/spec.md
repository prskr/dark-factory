# Spec Delta

## MODIFIED Requirements

### Requirement: Rewrites pass the push checks
A rewrite SHALL pass the push checks like any other push: rewritten commits are new commits that keep their author, so rewriting commits whose author is not the run's persona identity is rejected with `df:identity`. This includes commits of other personas, for example from a base task.

#### Scenario: Rebase of third-party commits
- **WHEN** the policy is `with-lease` and the agent of `DF-13` rebases `renovate/serde-1.x` onto a moved `main`, rewriting a commit authored by Renovate
- **THEN** the push is rejected with `df:identity`, the report has `REJECTED_BY_CHECK`, and the remote branch is unchanged

#### Scenario: Rebase of another persona's commits
- **WHEN** the policy is `with-lease` and the agent of `DF-14` (persona `reviewer`) rewrites a commit authored by persona `linus`
- **THEN** the push is rejected with `df:identity` and the remote branch is unchanged
