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

### PR Scope

Prefer PRs that map to one complete, user-understandable capability. Roadmap milestone bullets are the default PR boundary: a PR should usually complete one bullet. Closely related small bullets may be grouped into one PR, and unusually large bullets may be split into several focused PRs. Documentation, examples, and small helper changes should usually travel with the feature they explain.

Good standalone PR boundaries:

- a new public SDK capability
- a risky protocol or transport behavior change
- a structural refactor that would obscure a feature diff
- a release-only version bump and publish preparation
- a focused bug fix or compatibility fix

Avoid standalone PRs for:

- small roadmap wording updates
- example-only polish that belongs with a feature
- tiny helper functions without independent user value
- workflow experiments unless the repository policy itself is changing

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

Before opening a release PR or declaring it merge-ready, review the full release range relative to the previous release tag, such as `v0.2.0..HEAD` for `v0.3.0`. The release notes or PR body should summarize the user-visible changes in that range, not only the release branch metadata diff.

After the release PR is merged back to `main`, tag the resulting `main` commit and publish from that commit. Do not tag or publish from the release branch before it is merged.

Keep annotated tag messages short, for example `Release v0.2.0`. Put release highlights, links, and migration notes in the GitHub Release instead.

Current manual release flow:

1. Cut a short `release/<version>` branch from the latest `main`.
2. Make release-only changes, such as version, metadata, README, or release notes updates.
3. Review the full release range from the previous tag through the release branch head.
4. Open a release PR and wait for CI, including `Release dry run`, to pass.
5. Merge the release PR back to `main`.
6. Update local `main` to the merged commit.
7. Verify the merged commit with `cargo publish --dry-run`.
8. Create and push an annotated tag, for example `v0.2.0`.
9. Run `cargo publish` from the tagged `main` commit.
10. Confirm the crate version is visible on crates.io.
11. Create a GitHub Release from that tag.
12. Delete the release branch when it is no longer useful.

Trusted publishing and tag-triggered release automation may be added later.
