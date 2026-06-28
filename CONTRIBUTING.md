# Contributing

This project is an early community Rust implementation aligned with the Lark/Feishu Channel SDK family. The repository is still stabilizing, so contribution rules are intentionally lightweight.

## Branches

Use short typed branch prefixes:

- `feat/<name>` for new functionality
- `fix/<name>` for bug fixes
- `docs/<name>` for documentation-only changes
- `ci/<name>` for workflow changes
- `chore/<name>` for project maintenance
- `refactor/<name>` for behavior-preserving code changes
- `release/<version>` for release preparation

The default branch is `main`. Feature work should happen through pull requests.

## Commits

Use Conventional Commit-style messages:

- `feat: add app token client`
- `fix: handle api error payloads`
- `docs: update roadmap`
- `ci: add Rust workflow`
- `chore: prepare release`
- `refactor: split outbound message types`

Keep commits focused. Draft PRs may collect multiple review and iteration commits, especially while the design is still moving.

Avoid force-pushing away reviewed commits unless the branch needs a deliberate cleanup and reviewers can easily re-check the changed range.

Merge ready PRs with GitHub's **Squash and merge** option. This keeps `main` to one commit per PR while preserving the development commits, review discussion, and iteration history on the PR page.

Use the squash commit title as the final changelog-quality summary, usually matching the PR title:

```text
feat: add app token client (#12)
```

If a PR contains several meaningful development commits, preserve their subjects in the squash commit body:

```text
* feat: add token request types
* feat: implement app token client
* docs: add app token example
```

## Pull Requests

Open draft PRs for larger work while the shape is still changing. Mark a PR ready for review when:

- the implemented scope is clear
- README/docs match the current behavior
- CI passes
- new behavior has tests or a clear reason tests are deferred

PR descriptions should include:

- summary of changes
- what is intentionally out of scope
- verification commands, usually `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features`

## Review Responses

The following practices are recommendations rather than hard requirements. When addressing review feedback, prefer replying in the related review thread so the decision stays close to the original comment.

- If a change fixes the comment, mention the fixing commit when useful, for example `Fixed in abc1234`.
- If one commit fixes multiple related comments, reply to each relevant thread with the same commit hash and a short note.
- If the feedback is deferred or intentionally not changed, explain the tradeoff in the thread and resolve it when the decision is clear.
- Keep fix commits grouped by intent rather than by individual comments.

## Releases

Prepare releases on short `release/<version>` branches cut from the latest `main`.

Release PRs should contain only release preparation changes:

- update `Cargo.toml` version
- update `CHANGELOG.md` or release notes
- make small package metadata or README fixes needed for publishing

Release PRs run an additional CI job with `cargo package` and `cargo publish --dry-run`.

After the release PR is merged back to `main`, tag the resulting `main` commit and publish from that commit. Do not tag or publish from the release branch before it is merged.
