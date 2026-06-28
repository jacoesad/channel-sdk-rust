# Examples

Examples show how applications can call the SDK from ordinary Rust code.

## Token request

`tokens.rs` requests both app and tenant access tokens with the default reqwest transport.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
cargo run --example tokens
```

For local development, you can keep these variables in a git-ignored `.env` file and load it in your shell before running the example:

```bash
set -a
source .env
set +a
cargo run --example tokens
```

The example prints token lengths only. It does not print token values.
