# Publish a crate with GitHub Trusted Publishing

`.github/workflows/publish.yml` publishes the package from an existing
stable GitHub release. It uses GitHub OIDC and a short-lived crates.io token;
no persistent crates.io token is stored in repository secrets. GitHub binary
releases continue to use the existing Release workflow.

## One-time setup

1. Sign in as an owner of the existing `log-analyzer` crate.
2. Include the Cargo description, MIT license declaration and `LICENSE` file in
   the reviewed release before creating its tag. Older tags without this metadata
   fail validation. Do not move a published tag to retrofit these changes.
3. Create the GitHub environment `crates-io`. Configure its deployment branch
   policy to allow `main`; add any desired required reviewers.
4. In the crate's crates.io settings, register a GitHub trusted publisher:
   - Repository owner: `eirenik0`
   - Repository: `log-analyzer`
   - Workflow filename: `publish.yml`
   - Environment: `crates-io`

Crates.io must associate this exact workflow/environment identity with a crate
you own. Setting up the GitHub environment alone does not establish that trust.
Follow the current [crates.io setup guide](https://crates.io/docs/trusted-publishing)
when registering the publisher.
See the [official authentication action](https://github.com/rust-lang/crates-io-auth-action)
for the OIDC exchange and automatic token revocation.

## Publish a version

Create the reviewed stable GitHub release and tag through the existing Release
workflow. Then dispatch **Publish crate** from `main`, setting `version` to the
same version without `v` (for example `0.3.0`). Leave `dry-run` enabled first.

Validation requires a published, non-prerelease GitHub release, checks the tag's
Cargo version and required package metadata, and runs `cargo publish --dry-run`
with package verification enabled. The dry run does not request an OIDC token or
upload a crate. It does not validate registry ownership or Trusted Publisher
configuration.

After a successful dry run, dispatch again with the same version and `dry-run`
disabled. The publishing job uses the exact commit validated in that run and
obtains its token only after validation succeeds. It publishes the Cargo source
package to crates.io; GitHub binary archives remain separate assets.

A version already published to crates.io cannot be replaced. A retry of an
already-published version fails visibly rather than claiming another successful
publication. A moved tag is resolved and revalidated on each new dispatch.
