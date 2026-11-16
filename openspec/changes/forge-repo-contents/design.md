# Design

## Context

- **Plugins:** `crates/df-plugins` loads components (`host.rs`), wraps calls per interface (`forge.rs`), converts types (`convert.rs`) and routes by project (`router.rs`). Capability detection already reports exported interfaces as flags. Each call runs on a fresh instance with a 30 s default timeout and 64 MiB memory. The host doesn't limit HTTP response bodies.
- **Domain:** `ForgeClient` (`crates/df-core/src/seams.rs`) is keyed by `&ProjectId`. `IntegrationError` mirrors `types.error` one to one.
- **`iter3-ci-tools`** (in flight) adds the optional `ci` interface and an `unsupported(string)` error. This change follows the same pattern and shares that error.
- **Forge facts** (checked 2026-11-15):
  - **GitHub contents API:**
    - At most 1,000 entries per directory, with no signal that the list was cut.
    - Base64 content up to 1 MB; `application/vnd.github.raw+json` returns raw bytes up to 100 MB.
    - GitHub follows a symlink whose target is a regular file: its response describes the target file, not the link. The plugin still returns the link target (D3).
    - Submodules carry `submodule_git_url`.
    - `GET /commits/{ref}` with `Accept: application/vnd.github.sha` returns only the SHA. `ref` accepts a SHA, `heads/<branch>` or `tags/<tag>`.
  - **Forgejo `GET /repos/{o}/{r}/contents/{path}?ref=`:**
    - A directory returns an array, a file an object.
    - `type` is `file`, `dir`, `symlink` or `submodule`, with `size`, `sha`, `target` and `submodule_git_url`.
    - File content is base64.

## Goals / Non-Goals

**Goals:**
- One snapshot per read: resolve first, read at the resolved commit.
- No new crate dependency (base64 is already used by the plugins; the implementer checks `forge-common`).

**Non-Goals:**
- Caching reads in the host: rejected. Reads at a commit are immutable, but no consumer needs a cache yet. Consumers that do (the composer) cache themselves.
- Git LFS resolution: backlog if the image builder needs it (`roadmap.md`, image builder row).

## Decisions

### D1: WIT

```wit
interface contents {
    use types.{repo-ref, error};
    variant revision { branch(string), tag(string), commit(string) }
    enum entry-kind { file, dir, symlink, submodule }
    /// `sha`: Git object id (blob for files and symlinks, tree for dirs, commit for submodules).
    record entry { name: string, path: string, kind: entry-kind, size: option<u64>, sha: string }
    record listing { commit: string, entries: list<entry>, truncated: bool }
    variant blob { file(list<u8>), symlink(string), submodule }
    record file { commit: string, sha: string, size: u64, blob: blob }
    /// One directory level; at most 1,000 entries. Empty path = root.
    list: func(repo: repo-ref, rev: revision, path: string) -> result<listing, error>;
    /// One file; content at most 1 MiB, else `too-large`.
    get-file: func(repo: repo-ref, rev: revision, path: string) -> result<file, error>;
}
```

- `types.error` gains `too-large(u64)`, the file size in bytes.
- World `forge` exports `contents`.
- Paths are repository-relative, `/`-separated and without a leading `/`. The host rejects `..` segments, a leading `/` and backslashes with `invalid-input` before calling the plugin; plugins percent-encode each segment.
- `commit` is always the full SHA.
- Alternative: one `read(path)` that returns a directory or a file. Rejected: callers always know what they want, and two functions keep the result types flat.

### D2: Revision resolution per forge

| Revision | Forgejo | GitHub |
|---|---|---|
| `branch(b)` | `GET /branches/{b}` → `commit.id` | `GET /commits/heads/{b}` (sha media type) |
| `tag(t)` | `GET /tags/{t}` → `commit.sha` | `GET /commits/tags/{t}` (sha media type) |
| `commit(s)` | `GET /git/commits/{s}` → `sha` | `GET /commits/{s}` (sha media type) |

The plugin then reads `contents/{path}?ref=<sha>`. Each read costs two forge calls. Alternative: pass the ref straight to `?ref=` and skip resolving. Rejected: then the named commit could differ from the content read, and a branch and a tag with the same name would be ambiguous.

### D3: File reads

- **Forgejo:** one contents call. If `size` exceeds 1 MiB, return `too-large` without decoding. Otherwise base64-decode `content`. `symlink` returns `target`; `submodule` returns the marker.
- **GitHub:** one contents call with `application/vnd.github.object+json`.
  - For a file over 1 MiB (by `size`), return `too-large`.
  - At most 1 MiB the content is inline base64.
  - **Followed symlink:** when the response `path`, percent-decoded and without trailing `/`, differs from the requested path, the request may have hit a symlink that GitHub followed. The plugin then lists the parent directory and looks up the requested name. Only if that entry has kind `symlink` does it read the target text from `GET /git/blobs/{sha}`; otherwise it returns `internal` naming both paths.
  - A symlink to a directory or outside the repository comes back as `type: symlink` with `target`.
- **Host:** checks again that `file` content is at most 1 MiB, and fails with `too-large` otherwise. A buggy plugin can't push more into the control plane.

### D4: Listings

- **Forgejo:** the contents array is cut at 1,000 entries; `truncated` when it was longer.
- **GitHub:** exactly 1,000 entries returned means `truncated: true`. A directory with exactly 1,000 entries is reported truncated; that's accepted.
- Entries are sorted by name in both plugins, so results are stable for tests and the composer.

### D5: Host API

`ForgeClient` gains:

```rust
async fn list_repo_dir(&self, p: &ProjectId, rev: &Revision, path: &str) -> Result<RepoListing, IntegrationError>;
async fn read_repo_file(&self, p: &ProjectId, rev: &Revision, path: &str) -> Result<RepoFile, IntegrationError>;
```

- `Revision`, `RepoListing` and `RepoFile` are domain types in `df-core`, mirroring the WIT records.
- The router resolves the project's repository, as it already does for other calls.
- A plugin without `contents` fails with `unsupported("repository reads")` without calling the plugin.
- `FakeForgeClient` serves files from an in-memory map keyed by commit and path.
- Retries follow the existing per-contract rule (`rate-limited`, `transport`).

## Risks / Trade-offs

- **[Rate limits]** Two calls per read, and a walk costs one call per directory and file → consumers bound their walks (the image builder caps entries); host-side caching later if needed.
- **[Forgejo sends base64 content of large files]** A large file is transferred once before `too-large` → the plugin memory limit (64 MiB) bounds it, and files in `.dark-factory/` are small. Forgejo's own blob size cap applies above that.
- **[GitHub symlink detection by path mismatch]** It relies on GitHub returning the target's `path` → covered by plugin tests with recorded responses, including a path with percent-encoded characters; the fallback costs two extra calls only for symlinks.
- **[`unsupported` defined twice]** `iter3-ci-tools` adds the same error → whichever change lands second reuses it.

## Migration Plan

No data migration. The `too-large` error variant changes the shared `error` type, so plugin components built against the old contract no longer type-check against the host. The Forgejo, GitHub and test-probe plugins are rebuilt in the same change; third-party plugins don't exist yet (pre-MVP).
