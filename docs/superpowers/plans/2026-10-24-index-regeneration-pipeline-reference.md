# Index regeneration pipeline — verified reference (2026-10-24)

Facts and snippets gathered for the implementation plan (`2026-10-24-index-regeneration-pipeline.md`), each re-checked by a second researcher against primary sources (release pages, source code at the cited tags, official docs). **None of the Nextflow, nf-test, Docker or AWS snippets has been executed yet**: the plan's tasks run them first and fix what fails. Treat this file as a starting point, not as tested code.


## Pins and versions

### Findings

- **PIN TABLE (summary; release on or before 2026-10-10 unless marked). Re-checked 2026-10-24.** (verified): CONFIRMED except one correction (Debian 12.15 date; see the Debian finding). Nextflow 26.04.7 (stable, 2026-10-08). bioconda nextflow=26.04.7, build h2a3209d_0 (noarch, uploaded 2026-10-08). nf-test 0.9.5 (2026-03-19); bioconda nf-test=0.9.5, build h2a3209d_0 (noarch). nf-schema plugin 3.0.0 (2026-09-29): requires Nextflow >=26.04.0 and accepts JSON Schema draft 2020-12 only. Fallback nf-schema 2.8.0 (2026-08-10, same requirement). conda-forge openjdk=21.0.10 (current builds _22, uploaded 2026-08-28). AWS CLI v2 2.37.12 (2026-10-09). rust:1.94.1-bookworm (pushed 2026-04-08; Rust 1.94.1 released 2026-03-26). debian:bookworm-20261005-slim (pushed 2026-10-06; this is Debian 12.15). The floating debian:bookworm-slim tag was last pushed 2026-10-21, too new under the two-week rule.  
  Source: See the per-item findings below
- **Nextflow stable: chosen pin** (verified): CONFIRMED. v26.04.7 is the newest stable (not edge, not prerelease) release published on or before 2026-10-10: published 2026-10-08T16:28:35Z. The VERSION file at the tag reads 26.04.7. Release assets are 'nextflow' and 'nextflow-26.04.7-dist'. The dist URL redirects (302) and then returns 200 with content-length 42,440,978. The release notes include 'Fix S3 directory upload skipping symbolic links (#7515)' and 'Bump nf-amazon@3.9.3'.  
  Source: https://api.github.com/repos/nextflow-io/nextflow/releases?per_page=100 ; https://github.com/nextflow-io/nextflow/releases/tag/v26.04.7 ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/VERSION ; https://github.com/nextflow-io/nextflow/releases/download/v26.04.7/nextflow-26.04.7-dist
- **Nextflow: newer releases that are too new** (verified): CONFIRMED. These are not eligible under the two-week rule: v26.10.0 (2026-10-12T10:52Z), v26.10.1 (2026-10-15T13:11Z), v26.10.2 (2026-10-15T18:29Z), v26.10.3 (2026-10-20), v26.04.9 (2026-10-22), v25.10.9 (2026-10-20). v26.04.8 (2026-10-20) is marked prerelease, and its notes say: 'This release pins nf-k8s@1.5.6, which requires Nextflow 26.08.0-edge or later and fails to load on 26.04.x. Use 26.04.9 instead.' 26.10.0 becomes eligible on 2026-10-26. Edge releases (for example 26.09.2-edge, 2026-10-06) are excluded.  
  Source: https://api.github.com/repos/nextflow-io/nextflow/releases?per_page=100 ; https://github.com/nextflow-io/nextflow/releases/tag/v26.04.8
- **Nextflow 26.04.7: known security advisory** (verified): CORRECTED (which release notes say what). Only the v26.04.8 notes carry the advisory. They say Bitbucket Cloud API tokens used on 26.04.0 through 26.04.7 may have been written to console output, .nextflow.log files and the .git/config of cloned pipelines (#7868), and that such tokens should be rotated. The fix is #7870. The v26.04.9 notes do not mention Bitbucket; they list only 'Improve redaction of sensitive values in logs (#7879)' and two plugin bumps. However, the GitHub compare v26.04.7...v26.04.9 (13 commits ahead) includes 72e825ef1 'Keep Bitbucket API token out of git clone URL and logs (#7870) [26.04.x backport]', so 26.04.9 does contain the fix. Only users who authenticate to Bitbucket with an API token are affected. This pipeline runs from a local path ('nextflow run pipeline'), so it is not affected. The fixed 26.04.9 (2026-10-22) is too new to pin.  
  Source: https://github.com/nextflow-io/nextflow/releases/tag/v26.04.8 ; https://github.com/nextflow-io/nextflow/releases/tag/v26.04.9 ; https://api.github.com/repos/nextflow-io/nextflow/compare/v26.04.7...v26.04.9
- **Nextflow: Java requirement and manifest syntax** (verified): CONFIRMED. docs/install.md at v26.04.7 (line 15) says Nextflow requires 'Java 17 (or later, up to 26)'. docs/reference/config.md (lines 1284-1295) documents manifest.nextflowVersion forms including '>=1.2' and '!>=1.2', where the '!' prefix means 'stop execution if current version does not match required version'. So '!>=26.04.7' enforces the floor. Note that bioconda's own constraint, 'openjdk >=17,<26', excludes Java 26, which the docs allow; Java 21 satisfies both.  
  Source: https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/docs/install.md ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/docs/reference/config.md
- **Nextflow 26.04.7: default syntax parser and CLI param typing (affects the --help snippet)** (verified): NEW. In NF.groovy at v26.04.7, getSyntaxParserVersion() returns SysEnv.get('NXF_SYNTAX_PARSER', 'v2'), so the strict v2 parser is the default. In CmdRun.parseParamValue(), when NXF_DISABLE_PARAMS_TYPE_DETECTION is set or the v2 parser is active, a CLI value is returned as a String. Only under the v1 parser are 'true' and 'false' converted to Boolean. The 'exit' function is still documented in docs/reference/stdlib-namespaces.md but marked deprecated since 22.06.0-edge, with error() suggested instead.  
  Source: https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/modules/nextflow/src/main/groovy/nextflow/NF.groovy ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/modules/nextflow/src/main/groovy/nextflow/cli/CmdRun.groovy (lines 789-803) ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/docs/reference/stdlib-namespaces.md
- **bioconda nextflow package** (verified): CONFIRMED. bioconda nextflow 26.04.7 is noarch/nextflow-26.04.7-h2a3209d_0.conda: noarch generic, label main, uploaded 2026-10-08 23:52:31 UTC, 37,948,718 bytes. It depends on coreutils, curl and 'openjdk >=17,<26'. sha256 f229ca183e65424a1a8efa17985000c89bbf0c7edafc11c9fc9cbae8b7fb2cc6. Only this one noarch build exists for 26.04.7, and a noarch package installs on both osx-arm64 and linux-64. HEAD on the conda.anaconda.org URL returned 200 with content-length 37,948,718. Newer bioconda versions exist (26.10.0, 26.10.2, 26.10.3), but they track the too-new upstream releases.  
  Source: https://api.anaconda.org/package/bioconda/nextflow/files ; https://conda.anaconda.org/bioconda/noarch/nextflow-26.04.7-h2a3209d_0.conda
- **nf-test release** (verified): CONFIRMED, WITH ADDITIONS. nf-test v0.9.5 is the latest release, published 2026-03-19T15:42:32Z. Its notes cover topic-channel testing (#355), automatic updating of outdated plugin JSON (#356) and a path-agnostic shebang (#345). The asset nf-test-0.9.5.tar.gz redirects (302) and then returns 200 with 16,757,768 bytes. v0.9.4 (2026-02-12) added strict-syntax support (#336) and Java 25 testing (#337). Of the 22 issues and PRs updated since 2026-04-01, none reports an incompatibility with Nextflow 26.04. NEW: open issue #370 (2026-10-09) reports a user running nf-test 0.9.5 with Nextflow 26.04.7 on macOS arm64 with Temurin JDK 25. That shows the pair is used in practice. The issue itself is a '--shard' bug: concurrent 'nf-test test --shard i/N' processes in the same project directory delete each other's .nf-test-<hash>.nf launcher files, which causes intermittent NoSuchFileException failures. The plan should not run concurrent shards from one directory. No source states 0.9.5 to 26.04 compatibility explicitly. nf-test 0.9.5 predates Nextflow 26.04.0 (2026-04-29).  
  Source: https://api.github.com/repos/askimed/nf-test/releases ; https://github.com/askimed/nf-test/releases/tag/v0.9.5 ; https://api.github.com/repos/askimed/nf-test/issues?state=all&since=2026-04-01T00:00:00Z ; https://github.com/askimed/nf-test/issues/370
- **bioconda nf-test package** (verified): CONFIRMED. bioconda nf-test 0.9.5 is noarch/nf-test-0.9.5-h2a3209d_0.conda: noarch generic, label main, uploaded 2026-03-19 16:50:49 UTC. It depends on coreutils, curl and 'openjdk >=17,<26'. sha256 a99f62b01c74b01854c01bdebe765110f4754e2245ec536df10a521ad96befd5. HEAD returned 200 with 16,542,781 bytes. Being noarch, it installs on osx-arm64 and linux-64. 0.9.5 is the newest version on bioconda.  
  Source: https://api.anaconda.org/package/bioconda/nf-test/files ; https://conda.anaconda.org/bioconda/noarch/nf-test-0.9.5-h2a3209d_0.conda
- **nf-schema plugin: version and Nextflow minimum** (verified): CONFIRMED. The latest nf-schema is 3.0.0: GitHub release published 2026-09-29T13:58:33Z, registry date 2026-09-29T13:58:24Z. The registry reports requires '>=26.04.0', and build.gradle at tag 3.0.0 has nextflowPlugin { nextflowVersion = '26.04.0' }. Other registry versions: 2.8.0 (2026-08-10, '>=26.04.0'), 2.7.3 (2026-07-07, '>=25.10.0') and 2.7.2 (2026-04-17, '>=25.10.0'). Both 3.0.0 and 2.8.0 satisfy their requirement under Nextflow 26.04.7.  
  Source: https://registry.nextflow.io/api/v1/plugins/nf-schema ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/build.gradle ; https://api.github.com/repos/nextflow-io/nf-schema/releases
- **nf-schema 3.0.0 breaking changes (they affect the plan's --help)** (verified): CONFIRMED, WITH CLARIFICATIONS. Per the 3.0.0 migration guide:
- Automatic help via validation.help.enabled was removed and the option 'does no longer serve any purpose'. Help must come from paramsHelp().
- paramsHelp('input') became paramsHelp(parameter: 'input').
- paramsSummaryMap(workflow) and paramsSummaryLog(workflow) became paramsSummaryMap() and paramsSummaryLog().
- validation.failUnrecognisedParams and failUnrecognisedHeaders were removed in favour of validation { logging { unrecognisedParams = 'error'; unrecognisedHeaders = 'error' } }. LoggingConfig.groovy confirms these keys under 'logging'.
The statement that paramsHelp() 'doesn't stop pipeline execution' comes from docs/parameters/help_text.md, not the migration guide; that page also warns that 'exit' is not ideal (nextflow#3984). The official example (examples/paramsHelp/pipeline/main.nf) uses 'include { paramsHelp } from "plugin/nf-schema"', calls log.info paramsHelp(parameter: ['true','false'].contains(params.help) ? null : params.help, ...) and then 'exit 0'. Its nextflow.config pins 'nf-schema@3.0.0' and sets params.help = false. The validateParameters example is 'include { validateParameters } from "plugin/nf-schema"' followed by validateParameters() in the workflow.
NEW details from the 3.0.0 source:
- ParameterValidator always treats help, helpFull and showHidden as expected params, so unrecognisedParams = 'error' does not trip on them.
- The paramsHelp() option for a custom schema path is read as 'parameters_schema' in ValidationExtension.groovy and in ParamsHelpTest.groovy, but help_text.md documents it as 'parametersSchema'. This only matters if that option is passed.
- paramsHelp() converts 'parameter' to a String and throws SchemaValidationException if the name is not in the schema. So if params.help is Boolean true (the v1 parser, or a config default flipped to true), the example's ['true','false'].contains(params.help) is false, and the call would look up a parameter named 'true' and fail. Under 26.04.7's default v2 parser a CLI '--help' arrives as a String, so the official form works. Adding .toString() makes it work in both cases (inferred from the source; not executed, since Nextflow is not installed).  
  Source: https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/docs/3_0_0_migration_guide.md ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/docs/parameters/help_text.md ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/examples/paramsHelp/pipeline/main.nf ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/examples/paramsHelp/pipeline/nextflow.config ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/examples/validateParameters/pipeline/main.nf ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/src/main/groovy/nextflow/validation/ValidationExtension.groovy ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/src/main/groovy/nextflow/validation/help/HelpMessageCreator.groovy ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/src/main/groovy/nextflow/validation/parameters/ParameterValidator.groovy ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/src/main/groovy/nextflow/validation/config/LoggingConfig.groovy
- **nf-schema: JSON Schema draft** (verified): CONFIRMED AND STRENGTHENED (the colleague's open point is resolved). Draft 2020-12 is mandatory. JsonSchemaValidator.groovy at 3.0.0 (lines 49-59) reads '$schema' from the schema and, unless it equals exactly 'https://json-schema.org/draft/2020-12/schema', logs 'The used schema draft (...) is not correct' and throws SchemaValidationException. So draft-07, or even an http:// or trailing-'#' variant, is rejected. The v1 to v2 migration guide says 'The JSON schema draft has been updated from draft-07 to draft-2020-12' and that 'definitions' becomes '$defs'. The schema specification groups parameters under $defs and pulls them in with allOf [{"$ref": "#/$defs/<group>"}]. parameters_meta_schema.json declares draft 2020-12.  
  Source: https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/src/main/groovy/nextflow/validation/validators/JsonSchemaValidator.groovy ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/docs/migration_guide.md ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/docs/nextflow_schema/nextflow_schema_specification.md ; https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/parameters_meta_schema.json
- **conda-forge openjdk 21** (verified): CONFIRMED. The newest 21.x on conda-forge is 21.0.10; the versions list has no 21.0.11 or 21.0.12, and the package's latest_version is 25.0.2. There are two build sets.
- Build _0, uploaded 2026-02-18: osx-arm64 h258754b_0, osx-64 h48c29e7_0, linux-64 ha668962_0, linux-aarch64 h488f50d_0.
- Build _22, uploaded 2026-08-28: osx-arm64 hff8554d_22, linux-64 h1602c4f_22, linux-aarch64 hd4191a3_22, osx-64 h3ebe2c2_22. All are label main.
HEAD returned 200 for osx-arm64 openjdk-21.0.10-hff8554d_22.conda (181,007,997 bytes) and linux-64 openjdk-21.0.10-h1602c4f_22.conda (186,415,721 bytes). 21.0.10 satisfies 'openjdk >=17,<26', the constraint of both bioconda packages. 'openjdk=21.0.10' lets the solver pick the newest build.  
  Source: https://api.anaconda.org/package/conda-forge/openjdk ; https://conda.anaconda.org/conda-forge/osx-arm64/openjdk-21.0.10-hff8554d_22.conda ; https://conda.anaconda.org/conda-forge/linux-64/openjdk-21.0.10-h1602c4f_22.conda
- **Conda environment solvability** (likely): UNCHANGED (still not solved). On re-check, conda, mamba, micromamba and pixi are still absent from this machine, so no solve was run. Solvability is inferred only from package metadata: two packages are noarch, openjdk is native for osx-arm64 and linux-64, and both bioconda packages require 'openjdk >=17,<26', which 21.0.10 satisfies.  
  Source: Local check (command -v conda mamba micromamba pixi: none) ; anaconda.org file metadata above
- **AWS CLI v2: latest and pinned version** (verified): CONFIRMED. The latest AWS CLI v2 tag is 2.38.5 (commit 'Bump version to 2.38.5', 2026-10-22T18:09:41Z). The unpinned awscli-exe-linux-x86_64.zip currently has the same ETag (c0be003791a41edf29cdce604ae63689-9) and length (74,414,561) as the 2.38.5 zip. The newest release on or before 2026-10-10 is 2.37.12 (commit 'Bump version to 2.37.12', 2026-10-09T18:09:23Z), and CHANGELOG.rst at that tag starts with the 2.37.12 entry. Later releases are too new: 2.37.13 (2026-10-12), 2.38.0 (2026-10-13), 2.38.1 (2026-10-14), 2.38.2 (2026-10-19), 2.38.4 (2026-10-21) and 2.38.5 (2026-10-22).  
  Source: git ls-remote --tags https://github.com/aws/aws-cli.git ; https://api.github.com/repos/aws/aws-cli/commits/2.37.12 ; https://raw.githubusercontent.com/aws/aws-cli/2.37.12/CHANGELOG.rst
- **AWS CLI v2: pinned installer URLs (HEAD-checked)** (verified): CONFIRMED.
- https://awscli.amazonaws.com/awscli-exe-linux-x86_64-2.37.12.zip: 200, application/zip, 73,886,092 bytes, Last-Modified Fri 09 Oct 2026 19:05:44 GMT.
- https://awscli.amazonaws.com/awscli-exe-linux-aarch64-2.37.12.zip: 200, application/zip, 71,337,894 bytes, Last-Modified 19:07:29 GMT.
- Both .zip.sig files: 200, 566 bytes each.
- awscli-exe-linux-x86_64-9.99.99.zip: 404, so the pinned pattern is real and not a catch-all.
AWS's 'Installing past releases' page documents the same pattern (awscli-exe-linux-x86_64-2.0.30.zip and awscli-exe-linux-aarch64-2.0.30.zip).  
  Source: curl -I on the URLs listed ; https://docs.aws.amazon.com/cli/latest/userguide/getting-started-version.html
- **AWS CLI v2: checksums and signature** (likely): RE-CONFIRMED independently. I downloaded both 2.37.12 zips again into a fresh directory, /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/research/verify2/awscli/, and the sha256 values match the colleague's exactly. x86_64: e6d3c672fb36ed4b12c354c39a993eee2b34da1437cafb10bac4480840cfa83f. aarch64: 829c76096e05f743267d75098bc7cc7cfae799fc7e568554ebaa6d78e6aba753. AWS publishes no sha256, and the S3 ETags are multipart ('-9') rather than file hashes. The PGP .sig files were NOT verified because gpg is still not installed. So these hashes show only what awscli.amazonaws.com served over HTTPS twice, not a signature-verified file. AWS's install page gives the signing key fingerprint as FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C. The zip entries are dated 2026-10-09.  
  Source: Local download + shasum -a 256 ; https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html
- **AWS CLI v2: install prerequisites in the image** (verified): CORRECTED (groff and less). AWS's install page says you must be able to 'unzip' the package and that 'The AWS CLI uses glibc, groff, and less'. It does NOT say groff and less are needed only for 'aws help'; the colleague's wording is not from AWS. From the source at tag 2.37.12: awscli/utils.py OutputStreamFactory.get_output_stream() uses a pager (default less) only when 'is_a_tty() and pager', so non-interactive Nextflow tasks do not need less. awscli/help.py calls groff (or mandoc) to render help pages. So leaving groff and less out only breaks 'aws help' and interactive paging; setting ENV AWS_PAGER="" is a harmless extra safeguard. The spec's runtime package list has no unzip, so the plan needs unzip in the runtime stage or a separate install stage. The installer script in the zip (aws/install) installs to <install-dir>/v2/<version>, points the absolute symlink <install-dir>/v2/current at it, and links <bin-dir>/aws to <install-dir>/v2/current/bin/aws. The --install-dir/-i and --bin-dir/-b flags are documented. Copying /usr/local/aws-cli to the same path in another stage therefore keeps the links valid. Nextflow 26.04.7 docs/aws.md confirms that 'aws' is used 'to stage input files and output files between S3 and the task containers' and can come from the container image or a custom AMI (aws.batch.cliPath). It calls this 'a legacy requirement' and recommends Wave+Fusion.  
  Source: https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html ; https://docs.aws.amazon.com/cli/latest/userguide/cli-usage-pagination.html ; https://raw.githubusercontent.com/aws/aws-cli/2.37.12/awscli/utils.py (lines 414-435) ; https://raw.githubusercontent.com/aws/aws-cli/2.37.12/awscli/help.py (lines 249-276) ; aws/install inside awscli-exe-linux-x86_64-2.37.12.zip (lines 69-136) ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/docs/aws.md
- **Docker: rust:1.94.1-bookworm** (verified): CONFIRMED, WITH ONE DATE NOTE. The tag is active on Docker Hub, last pushed 2026-04-08T04:44:53Z.
- Index digest: sha256:6ae102bdbf528294bc79ad6e1fae682f6f7c2a6e6621506ba959f9685b308a55.
- linux/amd64: sha256:4ec71e955e6c08aeb238885083222ddff79d82eb87654a96c76e38e94da1a53b.
- linux/arm64/v8: sha256:55912ad77b52c411af5ecb4cf8901199b3688dbc96bf979570bbd8834dac69aa.
Rust 1.94.1 was released 2026-03-26 (GitHub release and RELEASES.md agree). The repo's rust-toolchain.toml has channel = "1.94.1". Newer Rust exists: 1.99.1 (GitHub release published 2026-10-16T04:47Z; RELEASES.md heading says 2026-10-15) and 1.99.0 (2026-10-01). The spec deliberately pins 1.94.1. The tag's Debian layer dates from April 2026, which only matters for the build stage. rust:1.94.1-slim-bookworm also exists: pushed 2026-04-07, index sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2, amd64 sha256:f845b308e3104231c98dba2dab952ebd54e35cd6ab8ef4a230ecb38e2315f20c, arm64/v8 sha256:c596ad3e4f5fbe2498d6f71498349691b9ee219288e298ec78f1f3dcd41124e8.  
  Source: https://hub.docker.com/v2/repositories/library/rust/tags/1.94.1-bookworm ; https://hub.docker.com/v2/repositories/library/rust/tags/1.94.1-slim-bookworm ; https://api.github.com/repos/rust-lang/rust/releases ; https://raw.githubusercontent.com/rust-lang/rust/master/RELEASES.md ; /Users/leipzig/Documents/dev/clingen/rust-toolchain.toml
- **Docker: debian bookworm-slim** (verified): CORRECTED (Debian 12.15 date). debian:bookworm-slim is a floating tag, last pushed 2026-10-21T01:36:49Z. Its index digest sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8 is the same as bookworm-20261020-slim and 12.15-slim.
The colleague's claim that 'Debian 12.15 was published as an image on 2026-10-21' is WRONG. In docker-library/official-images, library/debian first tagged 12.15-slim on bookworm-20260713-slim (commit 66ceaa32c0, 2026-07-13). Every rebuild since has carried 12.15-slim: 20260803, 20260824, 20260918, 20261005 and 20261020. Docker Hub shows only the tag's last push date. 12.14-slim was last applied to bookworm-20260623-slim (commit 9b0711ed60; pushed 2026-06-24).
The newest dated bookworm snapshot pushed on or before 2026-10-10 is debian:bookworm-20261005-slim, pushed 2026-10-06T02:24:26Z. official-images commit 6b287dc80f (2026-10-05) tags it 'bookworm-slim, bookworm-20261005-slim, 12.15-slim, 12-slim', so it is Debian 12.15.
- Index digest: sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587.
- amd64: sha256:a4672c0cb26fbdde88e38fa2dfb6c681942306680e41e4378b28770b6e79ee91.
- arm64/v8: sha256:a1b86db52ce3daef089e45aabe36dfec4091f82464c25c1fdcf03de197cbe82a.
Using bookworm for both build and runtime keeps glibc 2.36 in both stages; packages.debian.org currently lists bookworm libc6 2.36-9+deb12u14.  
  Source: https://hub.docker.com/v2/repositories/library/debian/tags/bookworm-slim ; https://hub.docker.com/v2/repositories/library/debian/tags/bookworm-20261005-slim ; https://hub.docker.com/v2/repositories/library/debian/tags?page_size=100&name=bookworm-2026 ; https://hub.docker.com/v2/repositories/library/debian/tags?page_size=100&name=12. ; https://raw.githubusercontent.com/docker-library/official-images/6b287dc80f/library/debian ; https://raw.githubusercontent.com/docker-library/official-images/66ceaa32c0335e6f89173798988f0a395af3b9e2/library/debian ; https://raw.githubusercontent.com/docker-library/official-images/9b0711ed609dceb3046d6606466747914a9caf03/library/debian ; https://packages.debian.org/bookworm/libc6
- **Debian bookworm support status** (verified): CONFIRMED. The Debian LTS wiki's schedule (last updated 2026-08-31; page last modified 2026-09-01) lists Debian 12 bookworm, released 2023-06-10, under LTS from 2026-06-11 to 2028-06-30, for i386, amd64, armhf, arm64 and ppc64el. Debian 13 trixie (released 2025-08-09) is listed for future LTS from 2028-08-09 to 2030-06-30. Regular Security Team support for bookworm has therefore ended, and LTS patches it through 2028-06-30.  
  Source: https://wiki.debian.org/LTS

### Open questions the researchers raised

- nf-schema 3.0.0 or 2.8.0? 3.0.0 (2026-09-29) is the newest eligible release, but it is a major release with breaking API changes and is 25 days old. With 3.0.0, --help must be written with paramsHelp() and the run must exit itself. 2.8.0 keeps the 2.x API and needs the same Nextflow >=26.04.0. Either way, nextflow_schema.json must declare draft 2020-12 exactly.
- Debian base: the two-week rule rules out the floating debian:bookworm-slim tag (last pushed 2026-10-21). The recommendation is the dated debian:bookworm-20261005-slim (Debian 12.15) plus its digest. The planner should confirm that a dated, digest-pinned tag is wanted rather than a floating tag that picks up LTS security fixes.
- The AWS CLI 2.37.12 PGP signatures were not verified because gpg is not available here. The sha256 values were reproduced by two independent HTTPS downloads but are not signature-verified. The plan could add a gpg check against AWS key fingerprint FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C at image build time.
- The spec's runtime package list has no 'unzip', which the AWS CLI installer needs. Add it to the runtime stage, or install the CLI in a separate stage as in the snippet? groff and less are not needed for non-interactive tasks (pager only on a TTY; groff only for help).
- No conda solve was run for environment.yml because no conda, mamba, micromamba or pixi is installed here. Compatibility was checked only from package metadata.
- nf-test 0.9.5 (2026-03-19) predates Nextflow 26.04.0. No incompatibility report exists, and issue #370 shows the pair in use, but no source states compatibility. Known bug #370: concurrent '--shard' runs from the same directory fail intermittently, so the plan should not shard nf-test in parallel from one checkout.
- conda-forge has no openjdk newer than 21.0.10. Whether 21.0.11 or 21.0.12 exist upstream was not checked.
- Nextflow 26.04.7 is in the range affected by the Bitbucket API-token log leak (26.04.0 to 26.04.7). The pipeline does not use Bitbucket, and the fixed 26.04.9 (2026-10-22) is too new to pin. Should the plan mention this anyway?
- Not executed: the main.nf --help snippet. Its behaviour (String vs Boolean params.help, exit under strict syntax) is inferred from the Nextflow 26.04.7 and nf-schema 3.0.0 source. Nextflow is not installed here, so the first nf-test run should cover '--help' and '--help chrom'.

### Snippets

#### pipeline/environment.yml (pins verified 2026-10-24; unchanged except comments)

```
name: clingen-index-pipeline
channels:
  - conda-forge
  - bioconda
  # use with: conda config --set channel_priority strict ; no 'defaults'
dependencies:
  - conda-forge::openjdk=21.0.10   # builds _22 (2026-08-28): osx-arm64 hff8554d_22, linux-64 h1602c4f_22
  - bioconda::nextflow=26.04.7     # noarch h2a3209d_0, released 2026-10-08; depends openjdk >=17,<26
  - bioconda::nf-test=0.9.5        # noarch h2a3209d_0, released 2026-03-19; depends openjdk >=17,<26
# Not solved locally (no conda/mamba/micromamba/pixi on this machine); compatibility inferred from metadata.
```

#### nextflow.config: version floor, plugin pin, help param (added params.help as in the official nf-schema 3.0.0 example)

```
manifest {
    nextflowVersion = '!>=26.04.7'   // '!' stops the run on a non-matching Nextflow
}

plugins {
    id 'nf-schema@3.0.0'            // requires Nextflow >=26.04.0; 2.8.0 is the pre-3.x fallback
}

params {
    help = false                    // as in nf-schema 3.0.0 examples/paramsHelp/pipeline/nextflow.config
}

validation {
    logging {
        unrecognisedParams = 'error'  // 3.x replacement for failUnrecognisedParams (help/helpFull/showHidden are always expected)
    }
}
```

#### main.nf: --help and parameter check with nf-schema 3.x (adapted from the official 3.0.0 examples)

```
include { paramsHelp; validateParameters } from 'plugin/nf-schema'

workflow {
    if (params.help) {
        // The official example uses ['true','false'].contains(params.help) without toString(). That works for
        // String values (26.04.7's default v2 parser keeps CLI values as Strings), but a Boolean true
        // (v1 parser or config) would be looked up as a parameter named 'true' and throw. toString() covers both.
        log.info paramsHelp(
            parameter: ['true', 'false'].contains(params.help.toString()) ? null : params.help,
            command: 'nextflow run pipeline -profile local --chrom 21'
        )
        exit 0   // paramsHelp() does not stop the run; 'exit' is deprecated (22.06.0-edge) but still available
    }
    validateParameters()
    // ...
}
// nextflow_schema.json must set "$schema": "https://json-schema.org/draft/2020-12/schema" (exact string; nf-schema 3.0.0
// throws on anything else) and group params under $defs, pulled in with allOf [{"$ref": "#/$defs/<group>"}].
```

#### Dockerfile base images and pinned AWS CLI v2 (sha256 re-computed independently; .sig not verified)

```
FROM rust:1.94.1-bookworm@sha256:6ae102bdbf528294bc79ad6e1fae682f6f7c2a6e6621506ba959f9685b308a55 AS build
# ... cargo build --release --locked -p clingen-index

# debian:bookworm-20261005-slim = Debian 12.15, pushed 2026-10-06
FROM debian:bookworm-20261005-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS awscli
ARG AWSCLI_VERSION=2.37.12
RUN set -eux; \
    apt-get update; apt-get install -y --no-install-recommends ca-certificates curl unzip; \
    arch="$(uname -m)"; \
    case "$arch" in \
      x86_64)  sha=e6d3c672fb36ed4b12c354c39a993eee2b34da1437cafb10bac4480840cfa83f ;; \
      aarch64) sha=829c76096e05f743267d75098bc7cc7cfae799fc7e568554ebaa6d78e6aba753 ;; \
      *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/awscliv2.zip "https://awscli.amazonaws.com/awscli-exe-linux-${arch}-${AWSCLI_VERSION}.zip"; \
    echo "${sha}  /tmp/awscliv2.zip" | sha256sum -c -; \
    unzip -q /tmp/awscliv2.zip -d /tmp; \
    /tmp/aws/install --install-dir /usr/local/aws-cli --bin-dir /usr/local/bin
    # installer layout: /usr/local/aws-cli/v2/2.37.12, absolute symlink v2/current -> it

FROM debian:bookworm-20261005-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
# apt-get install bash coreutils gawk gzip curl ca-certificates procps openssl ...
# groff/less omitted: only 'aws help' and TTY paging use them; AWS_PAGER="" is an extra safeguard
ENV AWS_PAGER=""
COPY --from=awscli /usr/local/aws-cli /usr/local/aws-cli
RUN ln -s /usr/local/aws-cli/v2/current/bin/aws /usr/local/bin/aws && aws --version
COPY --from=build /src/target/release/clingen-index /usr/local/bin/clingen-index
```


## Rust HTTP client (ureq, flate2)

### Findings

- **Recommendation summary** (verified): Confirmed, with one snippet bug fixed. Add to crates/clingen-index/Cargo.toml: ureq 3.4.2 with default features (rustls with the ring provider plus webpki-roots, and gzip), flate2 1.1.10 with default features (miniz_oxide), and serde_json "1" plus serde "1" with derive as direct dependencies. How I re-checked: I applied the pins in a copy of the repo's workspace manifests and Cargo.lock (stub sources; the repo itself is untouched). I then compiled the snippets verbatim with cargo/rustc 1.94.1 (aarch64-apple-darwin) against ureq 3.4.2, ureq-proto 0.6.4, rustls 0.23.45, rustls-webpki 0.103.15, webpki-roots 1.0.9, flate2 1.1.10 and serde_json 1.0.152. All 13 tests pass (the two snippet tests plus 11 of my own) and `cargo clippy --all-targets` is clean. A live HTTPS GET to reg.genome.network through the snippet's make_agent/enumerate_page returned 964 bytes holding 2 alleles. CORRECTION: the snippet's is_retryable retried TLS and certificate failures. Those arrive as Error::Io(InvalidData), not Error::Rustls (see 'Error kinds'). The snippet below is fixed. This machine has no cmake and no nasm (checked with `which`).  
  Source: Scratch project /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-proj (src/client.rs, src/fake.rs, src/extra.rs, src/gz.rs); workspace copy /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-ws; crates under /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/research/recheck-rust-http (CARGO_HOME=.../cargo-home)
- **ureq version** (verified): Confirmed. Recommended version: ureq 3.4.2, released 2026-09-13T13:47:40Z, MSRV 1.85, edition 2024; the crate's sha256 9a7ac20be9b7726e0bbdbf974c059676d9acb1cd414961f570a4e8231cacd7fc matches a fresh download. 3.4.3 (2026-10-21T17:56Z) and 3.5.0 (2026-10-21T18:15Z) are under two weeks old. 3.4.3's only change is 'Update TLS dependencies for security fixes #1218' (PR merged 2026-10-21): it raises rustls to ^0.23.46 and webpki-roots/webpki-root-certs to ^1.0.10. 3.4.2 requires rustls ^0.23.22, webpki-roots ^1.0.0 and ureq-proto ^0.6.3, so the lockfile alone decides which TLS fixes you get. 3.4.1 fixed timeout budgets (#1194, merged 2026-08-23) and completes the TLS handshake inside connect (#1193, merged 2026-08-23), so do not go below 3.4.1. ADDED: two known 3.4.2 limitations are fixed only in 3.5.0. (a) #1208 (merged 2026-10-16): during a TLS handshake each read or write renewed the whole timeout allowance, so a peer trickling bytes could stretch the handshake past timeout_connect or timeout_global. (b) #1205 (merged 2026-10-01): on Linux, a socket read with a timeout can return EINTR (ErrorKind::Interrupted), for example when a child process exits. In 3.4.2 that surfaces as Error::Io(Interrupted); the fixed is_retryable retries it.  
  Source: https://crates.io/api/v1/crates/ureq/versions ; https://crates.io/api/v1/crates/ureq/3.4.2/dependencies ; https://crates.io/api/v1/crates/ureq/3.4.3/dependencies ; https://static.crates.io/crates/ureq/ureq-3.4.2.crate (CHANGELOG.md) ; https://raw.githubusercontent.com/algesten/ureq/main/CHANGELOG.md ; https://github.com/algesten/ureq/pull/1218 , /pull/1194 , /pull/1193 , /pull/1208 , /pull/1205
- **ureq TLS provider** (verified): Confirmed. ureq 3.4.2's default features are ["rustls","gzip"]. The rustls feature turns on rustls-no-provider, _ring (rustls?/ring) and rustls-webpki-roots. ureq depends on rustls with default-features=false and features logging, std, tls12, so aws-lc-rs is not pulled in. build_config() in src/tls/rustls.rs (lines 137-158) picks a provider in this order: the one set in TlsConfig, then CryptoProvider::get_default(), then ring. TlsConfig::default() sets root_certs to RootCerts::WebPki (src/tls/mod.rs line 373), meaning the webpki-roots crate, Mozilla's list; the image's ca-certificates package is not used by the binary. In the resolved repo workspace, `cargo tree` shows ureq features _ring,_rustls,_tls,default,gzip,rustls,rustls-no-provider,rustls-webpki-roots, rustls features log,logging,ring,std,tls12, and ring 0.17.14. `cargo tree -i aws-lc-rs` reports that no such package exists. ADDED: src/lib.rs says ureq does not promise to keep ring as the default, so recheck on future upgrades.  
  Source: https://crates.io/api/v1/crates/ureq/versions (features of 3.4.2) ; https://crates.io/api/v1/crates/ureq/3.4.2/dependencies ; ureq-3.4.2/src/tls/rustls.rs lines 137-158 ; ureq-3.4.2/src/tls/mod.rs lines 152, 367-373 ; ureq-3.4.2/src/lib.rs lines 139-185 ; cargo tree in /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-ws
- **ring build requirements** (verified): Confirmed. ring 0.17.14 was released 2025-03-11T19:42Z, MSRV 1.66.0, and its sha256 a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7 matches crates.io. When built from crates.io (no .git directory), build.rs lines 340-358 use the pregenerated assembly, with the comment 'No Perl, no nasm, etc.'. Its only build dependency is cc (>=1.2.8, default-features=false). Its normal dependencies are cfg-if, getrandom 0.2 and untrusted 0.9. It built in my scratch project with Apple clang 21.0.0 on macOS arm64 without cmake or nasm. Docker Hub tag rust:1.94.1-bookworm: last pushed 2026-04-08T04:44:53Z; images for amd64, arm64/v8, arm/v7, 386, ppc64le and s390x. The docker-rust Dockerfile at the commit that set RUST_VERSION=1.94.1 (20c358f05538dc34ab5e2e586bd8f9229c49d51d) is FROM buildpack-deps:bookworm, which installs gcc, g++, make and libc6-dev. The aws-lc-rs requirements page says a non-FIPS build needs a C/C++ compiler and never needs CMake, bindgen or Go. Not done: no Docker build was run.  
  Source: https://crates.io/api/v1/crates/ring/versions ; https://static.crates.io/crates/ring/ring-0.17.14.crate (build.rs lines 340-358; Cargo.toml lines 242-254) ; https://hub.docker.com/v2/repositories/library/rust/tags/1.94.1-bookworm ; https://github.com/rust-lang/docker-rust/blob/20c358f05538dc34ab5e2e586bd8f9229c49d51d/stable/bookworm/Dockerfile ; https://raw.githubusercontent.com/docker-library/buildpack-deps/master/debian/bookworm/Dockerfile ; https://aws.github.io/aws-lc-rs/requirements/index.html
- **Transitive versions and the two-week rule** (verified): Confirmed, with additions. Adding the four dependencies to a copy of the repo's workspace and Cargo.lock on 2026-10-24 locked eight new crates newer than 2026-10-10: ureq 3.5.0 (10-21; the colleague listed this under 'Pinning approach'), ureq-proto 0.6.5 (10-17), rustls 0.23.46 (10-14), rustls-webpki 0.103.16 (10-20), webpki-roots 1.0.11 (10-23), crc32fast 1.5.3 (10-20), cc 1.10.0 (10-23) and find-msvc-tools 0.1.15 (10-19). The newest versions released on or before the cutoff are, with MSRV: ureq-proto 0.6.4 (2026-09-16, 1.85), rustls 0.23.45 (2026-09-14, 1.71), rustls-webpki 0.103.15 (2026-08-21, 1.71), webpki-roots 1.0.9 (2026-07-18, 1.70), crc32fast 1.5.2 (2026-09-12, 1.63), cc 1.7.0 (2026-10-10T01:21Z, 1.65.0) and find-msvc-tools 0.1.14 (2026-09-25, 1.65.0). New crates that already meet the rule: base64 0.23.1 (08-04), http 1.5.0 (07-29), httparse 1.10.1, log 0.4.34 (08-22), percent-encoding 2.3.2, rustls-pki-types 1.15.1 (07-23), utf8-zero 0.8.1, zeroize 1.9.1 (10-06, MSRV 1.85), miniz_oxide 0.9.1 (03-13), adler2 2.0.1, simd-adler32 0.3.10, untrusted 0.9.0, subtle 2.6.1, getrandom 0.2.17 (2026-01-11), bytes 1.12.1, shlex 2.0.1, zlib-rs 0.6.8 (lock only), plus wasi and windows-* crates (target-specific). Every MSRV is at or below 1.94.1. CORRECTION: the colleague's probe lockfile Cargo.lock.compliant is not fully compliant. It still holds libc 0.2.191 (2026-10-16), syn 3.0.9 (10-12) and unicode-ident 1.0.27 (10-12). In the repo, the new subtree uses libc through ring, then getrandom 0.2.17, on Unix, and resolves to the repo's existing libc 0.2.191. The newest libc on or before the cutoff is 0.2.190 (2026-10-02T19:33Z). Confirmed: the only change from ureq-proto 0.6.4 to 0.6.5 is raw header casing (new src/client/rawheaders.rs, header_raw); it is not a security fix.  
  Source: cargo metadata/update in /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-ws ; https://crates.io/api/v1/crates/<name>/<version> for every locked crate (saved in .../research/recheck-rust-http/lockdates/dates.tsv) ; https://crates.io/api/v1/crates/libc/versions ; diff of ureq-proto-0.6.4.crate vs ureq-proto-0.6.5.crate
- **Security conflict with the two-week rule** (verified): CORRECTED AND EXTENDED. Three releases the rule excludes are security fixes. (1) rustls 0.23.46 (2026-10-14) fixes RUSTSEC-2026-0346 (CVSS 4.0, MEDIUM: Debug output of client session values exposes secrets), RUSTSEC-2026-0347 (2.3 LOW: ECH application-data leak, affects only 0.23.44-0.23.45), RUSTSEC-2026-0348 (3.7 LOW: Brotli certificate decompression can allocate up to 1 GiB; Brotli is a non-default feature) and RUSTSEC-2026-0349 (3.7 LOW: an unauthenticated ServerHello can downgrade later connections to classical X25519 key exchange). (2) rustls-webpki 0.103.16 (2026-10-20) fixes RUSTSEC-2026-0382 / CVE-2026-111943: paths through distrusted emSign intermediates, whose private keys are public, could be accepted. (3) NEW: webpki-roots 1.0.9, the version the rule allows, is affected by RUSTSEC-2026-0381 (same CVE). It still bundles emSign roots (emSign Root CA - C1, emSign Root CA - G1, emSign ECC Root CA - G3); 1.0.10 (2026-10-19) removed ECC G3 and 1.0.11 (10-23) removed C1 and C3. rustls 0.23.45 already fixes RUSTSEC-2026-0285 (5.3 MEDIUM), so do not go lower. CORRECTED exposure reasoning: the Registry's real chain to Go Daddy Root Certificate Authority - G2 (re-checked with openssl s_client) does not protect against 0381/0382. The threat is a network attacker who presents a forged reg.genome.network certificate chained through a leaked emSign intermediate. The allowed set (webpki-roots 1.0.9 plus rustls-webpki 0.103.15) would accept it, and that could feed wrong CA IDs into the index. Either rustls-webpki 0.103.16 or webpki-roots 1.0.10 or later closes this. Exposure to 0349 is nil with ureq's ring provider: it offers only X25519, secp256r1 and secp384r1 (rustls 0.23.45 src/crypto/ring/mod.rs lines 177-181) and no X25519MLKEM768, so there is no post-quantum exchange to lose. ureq does not turn on Brotli or ECH, and the code does not Debug-print sessions. CORRECTED: going by the advisories' version ranges, cargo-audit or cargo-deny would flag six advisories against the allowed set (rustls 0346-0349, rustls-webpki 0382, webpki-roots 0381), not only 0382. Neither tool is installed here, so neither was run. I ran tests only against the allowed set, not against 0.23.46/0.103.16. Severities are computed from the advisories' CVSS 3.1 vectors; 0381 and 0382 carry no CVSS.  
  Source: https://raw.githubusercontent.com/rustsec/advisory-db/main/crates/rustls/RUSTSEC-2026-0346.md (also 0347, 0348, 0349, 0285) ; https://raw.githubusercontent.com/rustsec/advisory-db/main/crates/rustls-webpki/RUSTSEC-2026-0382.md ; https://raw.githubusercontent.com/rustsec/advisory-db/main/crates/webpki-roots/RUSTSEC-2026-0381.md ; https://github.com/rustls/rustls/releases/tag/v%2F0.23.46 ; https://github.com/rustls/webpki/releases/tag/v%2F0.103.16 ; https://github.com/rustls/webpki-roots/releases (v/1.0.10, v/1.0.11) ; webpki-roots-1.0.9.crate src/lib.rs ; rustls-0.23.45.crate src/crypto/ring/mod.rs
- **flate2** (verified): Confirmed. Recommended version: flate2 1.1.10, released 2026-08-28T04:35Z, MSRV 1.67.0; sha256 6e634e2e0ebac1ee034020da1ca582e17ffe4e0f5e985823721e168928136dcb matches a fresh download. Default features are [rust_backend, runtime_detection], and rust_backend means miniz_oxide (pure Rust). ureq's gzip feature already depends on flate2 ^1.0.30, so both unify to one copy (cargo tree shows 'flate2 v1.1.10 (*)' under ureq). zlib-rs 0.6.8 appears in Cargo.lock but is not compiled: `cargo tree -i zlib-rs` prints nothing. The lock entry fits the weak feature 'zlib-rs?/std' in runtime_detection. Versions 1.1.6 and 1.1.7 are yanked.  
  Source: https://crates.io/api/v1/crates/flate2/versions ; https://static.crates.io/crates/flate2/flate2-1.1.10.crate (Cargo.toml lines 14, 57-102) ; cargo tree in /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-ws
- **flate2: use MultiGzDecoder** (verified): Confirmed. data/clinvar/download/clinvar.vcf.gz is BGZF: its header bytes are 1f8b 0804 ... 06 00 'BC' 02 00. flate2::read::GzDecoder stops after the first gzip member and returned 45 lines. MultiGzDecoder returned 4,599,371 lines, the same count as `gzip -dc | wc -l`, in 1.88 s (release build, miniz_oxide, on this machine). The docs in src/gz/read.rs (lines 95-110 and 222-235) say GzDecoder reads 'a single member' and MultiGzDecoder 'decodes all members'. Use MultiGzDecoder everywhere; it also reads single-member files. ADDED: read::MultiGzDecoder::new already wraps its own 32 KiB BufReader, so the snippet's inner BufReader is redundant. bufread::MultiGzDecoder avoids the extra buffer, but timing was the same (1.90 s), so either is fine. I did not re-check the colleague's statement that gnomAD VCFs are bgzipped.  
  Source: xxd of /Users/leipzig/Documents/dev/clingen/data/clinvar/download/clinvar.vcf.gz (read-only) ; /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-proj/examples/gzcount.rs ; flate2-1.1.10/src/gz/read.rs lines 95-110, 143-150, 222-235, 268-276 ; src/bufreader.rs line 42
- **serde_json and the existing Cargo.lock** (verified): Confirmed. The repo's Cargo.lock has serde 1.0.229 (2026-07-18), serde_json 1.0.154 (2026-10-11T15:31Z, MSRV 1.71), fst 0.4.7 (2021-06-06), memmap2 0.9.12 (2026-10-21), anyhow 1.0.104 (2026-07-18), clap 4.6.7 (2026-09-14), thiserror 2.0.21 (2026-09-23), tempfile 3.27.0 (2026-03-11), vareffect 0.6.0 (2026-10-12) and libc 0.2.191 (2026-10-16). It has none of ureq, flate2, rustls or ring. serde_json and serde are direct dependencies of clingen-allele only, so clingen-index must declare them itself. Exactly six lock entries are newer than 2026-10-10: serde_json 1.0.154, memmap2 0.9.12, libc 0.2.191, syn 3.0.9, unicode-ident 1.0.27, and vareffect 0.6.0, which is hard-pinned with =0.6.0 in both crates. The newest serde_json on or before the cutoff is 1.0.152 (2026-10-10T17:04Z, MSRV 1.71); 1.0.151 (2026-07-20) is the conservative choice. Downgrading serde_json in the shared lock also changes it for clingen-allele.  
  Source: /Users/leipzig/Documents/dev/clingen/Cargo.lock ; /Users/leipzig/Documents/dev/clingen/crates/clingen-allele/Cargo.toml ; /Users/leipzig/Documents/dev/clingen/crates/clingen-index/Cargo.toml ; https://crates.io/api/v1/crates/serde_json/versions ; https://crates.io/api/v1/crates/<name>/<version> for every repo lock entry
- **ureq 3 config defaults** (verified): Confirmed, with two clarifications. Config::default() (src/config.rs lines 940-984) sets http_status_as_error: true, https_only: false, every timeout None except await_100 = 1 s, max_redirects 10, max_idle_age 15 s, max_idle_connections 10, max_idle_connections_per_host 3, and proxy: Proxy::try_from_env(). CLARIFIED proxy order: try_from_env checks ALL_PROXY, all_proxy, HTTPS_PROXY, https_proxy, HTTP_PROXY, http_proxy, in that order. The first valid one is used for every request whatever its scheme, so HTTPS_PROXY also proxies http:// URLs. NO_PROXY and no_proxy are honoured. On the wire, the default headers are 'user-agent: ureq/3.4.2' and 'accept-encoding: gzip'. timeout_global is documented as 'end-to-end, from DNS lookup to finishing reading the response body', and timeout_connect covers the TLS handshake. CLARIFIED: in 3.4.2 a trickling TLS handshake can exceed these budgets (fixed in 3.5.0, #1208). Before reusing a pooled connection, the pool probes it with a non-blocking read (pool.rs lines 286-296; transport/tcp.rs lines 287-300) and skips connections older than max_idle_age. The Registry sends Keep-Alive: timeout=10, so max_idle_age of about 8 s is an optional tweak.  
  Source: ureq-3.4.2/src/config.rs lines 491-528, 603, 633, 722-780, 833-865, 938-984 ; src/proxy.rs lines 215-267, 541-549 ; src/pool.rs lines 286-296 ; src/unversioned/transport/tcp.rs lines 287-300 ; recheck-proj test extra::default_headers_on_wire ; https://github.com/algesten/ureq/pull/1208
- **Body size limit** (verified): Confirmed. Body::read_to_string(), read_to_vec() and read_json() cap the body at MAX_BODY_SIZE = 10*1024*1024 (body/mod.rs line 30). To raise it, call resp.body_mut().with_config().limit(N).read_to_string(); into_with_config() is the owned form. BodyWithConfig defaults to limit u64::MAX and lossy_utf8 = false. Re-tested: a 12 MiB body with the default read gives Error::BodyExceedsLimit(10485760), and limit(13 MiB) succeeds. A 100-byte body fails with limit(100) (BodyExceedsLimit(100)) and succeeds with limit(101). LimitReader errors on the next read once its budget reaches 0, so the limit must be strictly larger than the body. The limit counts wire bytes before gzip decoding (reader stack MaybeLossyDecoder<CharsetDecoder<ContentDecoder<LimitReader<..>>>>, line 783): a gzip body of 5,116 wire bytes decoded to 5,242,880 bytes under limit(1 MiB). ADDED: invalid UTF-8 makes read_to_string fail with Error::Io(kind InvalidData), from std's read_to_string, not with a dedicated variant (tested).  
  Source: ureq-3.4.2/src/body/mod.rs lines 30, 292-400, 421-530, 579-584, 783 ; src/body/limit.rs ; recheck-proj tests extra::default_limit_10mib_and_exact_limit, extra::limit_counts_compressed_bytes, extra::invalid_utf8_body_is_io_invalid_data
- **Registry response format** (verified): Confirmed by a live request on 2026-10-24 (20:36 GMT). GET /alleles answers HTTP/1.1 200 with Content-Type: application/json, Transfer-Encoding: chunked, Content-Encoding: gzip, Keep-Alive: timeout=10, Vary: Accept-Encoding and X-CAR-Version: 5.1.0, and no Content-Length. Without a Content-Length, ureq's read_json (json feature) would use serde_json::from_reader (body/mod.rs lines 385-400; it uses from_slice only for a known small length). ureq issue #1151, 'read_json is bottlenecked by serde_json's reader API', describes the slowdown. Reading into a String and calling serde_json::from_str avoids it, so the json feature is not needed. The live call through the snippet's agent (ureq 3.4.2, ring, rustls 0.23.45, webpki-roots 1.0.9) to refseq=NC_000021.9&begin=33000000&end=33000050&limit=2 returned 964 bytes holding 2 alleles, each with only the keys @id and genomicAlleles, so the field filter was honoured. curl returned the same 964 bytes.  
  Source: curl -D - --compressed https://reg.genome.network/alleles?... on 2026-10-24 ; recheck-proj test extra::live_registry (LIVE=1) ; ureq-3.4.2/src/body/mod.rs lines 385-400 ; https://github.com/algesten/ureq/issues/1151
- **Query encoding** (verified): Confirmed on the wire. .query(k, v) and .query_pairs() percent-encode with ENCODED_IN_QUERY (query.rs lines 12-37), which includes '+', '@', ',', ':', '/', '=' and '&'. Sent request line: .query("f", "none+@id,a:b/c") became f=none%2B%40id%2Ca%3Ab%2Fc, and .query_raw("g", "none+@id") stayed g=none+@id. The snippet's fake-server test saw fields=none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome unchanged. harvest/query_registry.py lines 19-20 send a literal '+'. Use query_raw for fields, and query for refseq, begin, end, limit and skip. Untested: whether the Registry treats %2B the same as '+'.  
  Source: ureq-3.4.2/src/query.rs lines 9-37 ; src/request.rs lines 151-260 ; recheck-proj tests extra::query_vs_query_raw_on_wire and fake::retries_then_succeeds ; /Users/leipzig/Documents/dev/clingen/harvest/query_registry.py lines 19-20
- **POST body and Content-Type** (verified): Confirmed. RequestBuilder<WithBody>::send(impl AsSendBody) accepts String, &str, Vec<u8>, &[u8], &String, &Vec<u8>, File and others, and sets the size from the length. A String body adds no Content-Type (send_body.rs lines 309-314; content_type None); .content_type(v) sets one (request.rs lines 513-520). Wire-tested: a 79-byte String body sent content-length: 79 and no content-type; with .content_type(...) it sent content-type: application/x-www-form-urlencoded. The Python script sets no Content-Type, so urllib adds 'Content-type: application/x-www-form-urlencoded' (local CPython 3.9 urllib/request.py lines 1278-1281; current CPython main has the same code). Mirror that header. Untested against the Registry: other content types.  
  Source: ureq-3.4.2/src/request.rs lines 513-560 ; src/send_body.rs lines 290-420 ; recheck-proj test extra::post_string_headers_on_wire ; /Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/lib/python3.9/urllib/request.py lines 1278-1281 ; https://raw.githubusercontent.com/python/cpython/main/Lib/urllib/request.py
- **Status codes** (verified): Confirmed. With the default http_status_as_error(true), run.rs lines 124-129 turn 4xx and 5xx into Err(Error::StatusCode(u16)) and drop the body (tested: 503 gives StatusCode(503)). With .http_status_as_error(false), 4xx and 5xx return Ok(Response), and status and body read as usual (tested: 429 with body 'slow').  
  Source: ureq-3.4.2/src/run.rs lines 124-129 ; src/error.rs line 14 ; recheck-proj test extra::status_handling
- **Error kinds observed (ureq 3.4.2, macOS)** (verified): Confirmed, with a CORRECTION about TLS errors. Re-tested:
- Server accepts but never answers, timeout_global 500 ms: Error::Timeout(Timeout::Global).
- Headers sent, then the body stalls: read_to_string returns Error::Timeout(Timeout::Global). From<io::Error> unwraps ureq errors wrapped in io::Error (error.rs lines 212-227).
- Connection refused: Error::Io(kind ConnectionRefused).
- Server closes without a response: Error::Io(UnexpectedEof).
- Content-Length larger than the bytes sent: Error::Io(UnexpectedEof).
- Truncated JSON in a complete HTTP response: no ureq error; serde_json error with is_eof() == true.
- NEW, TLS failures: Error::Io(kind InvalidData) wrapping a rustls::Error. Measured: InvalidCertificate(UnknownIssuer) from a local openssl s_server with a self-signed v3 cert, and InvalidMessage(InvalidContentType) from a plain-HTTP peer. The cause: rustls_connector calls conn.complete_io(&mut sock)? (tls/rustls.rs line 78), rustls's complete_io wraps errors as io::Error::new(InvalidData, e) (rustls 0.23.45 conn.rs line 677), and ureq maps that to Error::Io. Error::Rustls comes only from ClientConnection::new and config building, never from handshake or certificate failures. So the colleague's 'Not retried: Rustls (cert errors)' was wrong, and the old matches!(E::Io(_)) retried certificate errors 6 times.
Error and Timeout are #[non_exhaustive]. Public Timeout variants: Global, PerCall, Resolve, Connect, SendRequest, SendBody, RecvResponse, RecvBody (Await100 is #[doc(hidden)]).  
  Source: recheck-proj tests extra::error_kinds, extra::tls_bad_cert_kind (TLS_SELF_SIGNED_URL=https://localhost:18443/), extra::tls_garbage_is_io_invalid_data ; ureq-3.4.2/src/tls/rustls.rs lines 37-90 ; src/error.rs lines 8-170, 212-227, 292-297 ; src/timings.rs lines 13-45 ; rustls-0.23.45/src/conn.rs lines 602-677
- **Fake HTTP server for unit tests** (verified): Confirmed, with a qualification on rule 1. A std::net::TcpListener on 127.0.0.1:0 with a thread serving one scripted reply per connection works with ureq 3.4.2 over http://, since https_only is false by default. 1. Read the whole request, including the Content-Length body, before replying. QUALIFIED: the failure only occurs with a large unread body. Over 20 runs each, replying and closing with the POST body unread gave the 503 every time (20/20) for 100-byte and 64 KiB bodies. With a 4 MiB body it never gave the 503: Io BrokenPipe (os 32) 8 times, ConnectionReset (os 54) 9 times, InvalidInput (os 22) 3 times. The colleague saw 11/6/3. Drain the body anyway. Even with the fixed is_retryable, those three kinds count as retryable, so a '503 is retried' test can still pass for the wrong reason. ureq PR #1220 'Preserve readable responses after macOS timeout setter failures' (merged 2026-10-22; 'Unreleased' in the main CHANGELOG; addresses #1219) covers only the macOS InvalidInput case. 2. Send 'Connection: close'. I did not re-run the colleague's keep-alive timing observation; the pool liveness probe it relies on is confirmed in pool.rs. 3. Build the test agent with .proxy(None) (ConfigBuilder::proxy takes Option<Proxy>). No *_proxy variable is set in this environment.  
  Source: recheck-proj tests extra::undrained_post_outcomes and src/fake.rs ; https://github.com/algesten/ureq/pull/1220 ; https://raw.githubusercontent.com/algesten/ureq/main/CHANGELOG.md ; ureq-3.4.2/src/config.rs line 528 ; src/pool.rs lines 286-296
- **Pinning approach** (verified): Confirmed in the repo context. With `ureq = "3.4.2"` in clingen-index/Cargo.toml, the first resolve against the repo's Cargo.lock locked ureq 3.5.0, along with the seven other post-cutoff crates listed above. These commands, run in this order, all applied cleanly, each printing a 'Downgrading' line: cargo update -p ureq --precise 3.4.2, ureq-proto 0.6.4, rustls 0.23.45, rustls-webpki 0.103.15, webpki-roots 1.0.9, crc32fast 1.5.2, cc 1.7.0, find-msvc-tools 0.1.14, serde_json 1.0.152. Commit Cargo.lock; the Dockerfile's `cargo build --release --locked` then reproduces it. The alternative is `=` pins in Cargo.toml, which the repo already uses for vareffect =0.6.0. libc is not touched by these commands and stays at 0.2.191 (2026-10-16).  
  Source: cargo +1.94.1 metadata and cargo update --precise output in /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/recheck-ws (copy of the repo's Cargo.toml, Cargo.lock, crates/*/Cargo.toml)

### Open questions the researchers raised

- Two-week rule vs security fixes (corrected). The allowed set has six open RustSec advisories: rustls 0.23.45 has RUSTSEC-2026-0346 (MEDIUM) and 0347/0348/0349 (LOW); rustls-webpki 0.103.15 has RUSTSEC-2026-0382; webpki-roots 1.0.9 has RUSTSEC-2026-0381 (the last two share CVE-2026-111943, emSign intermediates whose private keys are public). The Registry's Go Daddy G2 chain does not reduce the emSign exposure, because a man-in-the-middle presents its own chain. For this client, 0347/0348/0349 have no practical effect (no ECH, no Brotli, and ring has no post-quantum group), and 0346 needs Debug-printing of session values. The smallest exception would be webpki-roots 1.0.10 (2026-10-19), or rustls-webpki 0.103.16 (2026-10-20), to close the emSign issue; a full exception would be ureq 3.4.3 (2026-10-21), which requires rustls >=0.23.46 and webpki-roots >=1.0.10. The user needs to decide.
- Does the two-week rule cover the existing Cargo.lock? It already has serde_json 1.0.154 (10-11), memmap2 0.9.12 (10-21), libc 0.2.191 (10-16), syn 3.0.9 (10-12), unicode-ident 1.0.27 (10-12) and vareffect 0.6.0 (10-12). The new ring, then getrandom 0.2.17, subtree also uses libc 0.2.191; libc 0.2.190 (2026-10-02) is the newest on or before the cutoff. vareffect is hard-pinned with =0.6.0 in both crates.
- Untested against the live Registry: whether POST /alleles?file=vcf accepts a Content-Type other than application/x-www-form-urlencoded, and whether a percent-encoded fields value (%2B, %40) is treated the same as a literal '+' and '@'. The snippet uses the known-working form.
- No Docker build of rust:1.94.1-bookworm was run. The base image (FROM buildpack-deps:bookworm at docker-rust commit 20c358f0) and ring's needs (a C compiler only) were checked from sources, and ring built on macOS arm64 without cmake or nasm.
- The body limit counts compressed wire bytes, so it does not cap the decoded String. If decoded memory must be capped, the options are ConfigBuilder::accept_encoding("") (an empty string maps to AutoHeaderValue::None; the header is untested) or a counting reader wrapper.
- ureq 3.4.2 lacks two 3.5.0 fixes: a trickling TLS handshake can exceed timeout_connect/timeout_global (#1208), and on Linux EINTR during timed reads surfaces as Error::Io(Interrupted) (#1205; the fixed is_retryable retries it). Is that acceptable until 3.5.0 passes the two-week mark (2026-11-04)?

### Snippets

#### crates/clingen-index/Cargo.toml additions + lockfile pins (re-run in a copy of the repo workspace on 2026-10-24: all applied cleanly)

```
[dependencies]
# existing: anyhow, clap, clingen-allele, vareffect
flate2 = "1.1.10"                                 # 2026-08-28, MSRV 1.67.0, default = miniz_oxide
serde = { version = "1", features = ["derive"] }   # lock already has 1.0.229 (2026-07-18)
serde_json = "1"                                  # lock has 1.0.154 (2026-10-11)
ureq = "3.4.2"                                    # 2026-09-13, MSRV 1.85; defaults = rustls(ring, webpki-roots) + gzip

# then lock the newest versions released on or before 2026-10-10 (first resolve picks ureq 3.5.0):
cargo update -p ureq            --precise 3.4.2     # 2026-09-13
cargo update -p ureq-proto      --precise 0.6.4     # 2026-09-16
cargo update -p rustls          --precise 0.23.45   # 2026-09-14 (0.23.46 of 10-14 = RUSTSEC-2026-0346..0349 fixes)
cargo update -p rustls-webpki   --precise 0.103.15  # 2026-08-21 (0.103.16 of 10-20 = RUSTSEC-2026-0382 fix)
cargo update -p webpki-roots    --precise 1.0.9     # 2026-07-18 (1.0.10 of 10-19 = RUSTSEC-2026-0381 fix)
cargo update -p crc32fast       --precise 1.5.2     # 2026-09-12
cargo update -p cc              --precise 1.7.0     # 2026-10-10
cargo update -p find-msvc-tools --precise 0.1.14    # 2026-09-25
cargo update -p serde_json      --precise 1.0.152   # optional, 2026-10-10 (shared with clingen-allele)
# note: libc stays at the repo's 0.2.191 (2026-10-16); newest on/before the cutoff is 0.2.190
```

#### Registry client: agent, GET with query, POST with content type, raised body limit, status and error classification (compiled and tested: rustc 1.94.1, ureq 3.4.2, clippy clean; is_retryable CORRECTED so TLS/cert errors are not retried)

```
use std::time::Duration;
use ureq::Agent;

/// Cap on wire bytes (gzip is decoded after the limit). Must be strictly greater than the body.
const BODY_LIMIT: u64 = 1024 * 1024 * 1024;

pub fn make_agent(global_timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(global_timeout))            // DNS -> last body byte
        .timeout_connect(Some(Duration::from_secs(30)))  // includes TLS handshake
        .http_status_as_error(false)                     // 4xx/5xx come back as Ok(response)
        .user_agent(concat!("clingen-index/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

pub enum Outcome { Ok(String), Retry(String), Fail(String) }

fn finish(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Outcome {
    let mut resp = match resp {
        Ok(r) => r,
        Err(e) if is_retryable(&e) => return Outcome::Retry(e.to_string()),
        Err(e) => return Outcome::Fail(e.to_string()),
    };
    let status = resp.status().as_u16();
    let body = match resp.body_mut().with_config().limit(BODY_LIMIT).read_to_string() {
        Ok(b) => b,
        Err(e) if is_retryable(&e) => return Outcome::Retry(e.to_string()),
        Err(e) => return Outcome::Fail(e.to_string()),
    };
    match status {
        200..=299 => Outcome::Ok(body),
        429 | 500..=599 => Outcome::Retry(format!("HTTP {status}: {}", head(&body))),
        _ => Outcome::Fail(format!("HTTP {status}: {}", head(&body))),
    }
}

fn head(s: &str) -> &str { &s[..s.floor_char_boundary(300)] }

/// `fields` via query_raw so '+' and '@' go out unencoded, as harvest/query_registry.py sends them.
pub fn enumerate_page(agent: &Agent, base: &str, refseq: &str, begin: u64, end: u64, limit: u64, skip: u64) -> Outcome {
    finish(agent.get(format!("{base}/alleles"))
        .query("refseq", refseq)
        .query("begin", begin.to_string())
        .query("end", end.to_string())
        .query_raw("fields", "none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome")
        .query("limit", limit.to_string())
        .query("skip", skip.to_string())
        .call())
}

/// Read-only POST lookup; Content-Type mirrors what Python urllib sent by default.
pub fn lookup(agent: &Agent, base: &str, vcf: String) -> Outcome {
    finish(agent.post(format!("{base}/alleles"))
        .query("file", "vcf")
        .query_raw("fields", "none+@id+genomicAlleles.referenceGenome+genomicAlleles.hgvs")
        .content_type("application/x-www-form-urlencoded")
        .send(vcf))
}

/// Transport failures worth retrying. ureq::Error is #[non_exhaustive].
pub fn is_retryable(e: &ureq::Error) -> bool {
    use std::io::ErrorKind;
    use ureq::Error as E;
    match e {
        // TLS failures, including certificate errors (UnknownIssuer, expired, bad name), reach us
        // as Io(InvalidData) wrapping a rustls::Error, not as Error::Rustls; invalid UTF-8 in
        // read_to_string is Io(InvalidData) too. Both are deterministic: do not retry.
        E::Io(io) => io.kind() != ErrorKind::InvalidData, // refused, reset, EPIPE, UnexpectedEof, EINTR
        E::Timeout(_)          // any phase, incl. Timeout::Global while reading the body
        | E::HostNotFound
        | E::ConnectionFailed
        | E::Protocol(_)       // malformed HTTP/1.1 from the peer
        | E::Decompress(..) => true, // corrupt gzip stream
        // Not retried: BodyExceedsLimit, BadUri, Http, RequireHttpsOnly, TooManyRedirects,
        // RedirectFailed, Tls, StatusCode (cannot occur with http_status_as_error(false)).
        _ => false,
    }
}

// Parse after reading: serde_json::from_str::<Vec<Allele>>(&body). A truncated body that
// still arrived as complete HTTP gives a serde_json::Error with e.is_eof() == true.
// Treat that as retryable too, as the spec says.
```

#### Fake HTTP/1.1 server for unit tests (std only; tested with ureq 3.4.2 over http://127.0.0.1; needs the client items in scope)

```
use crate::registry::{enumerate_page, lookup, Outcome}; // wherever the client snippet lives

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Test agent: plain http is allowed by default (https_only = false); proxy(None) ignores env proxies.
fn make_test_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .http_status_as_error(false)
        .proxy(None)
        .build()
        .into()
}

/// Read request line, headers AND the whole Content-Length body before replying.
/// Replying with a large POST body unread (4 MiB tested) gives EPIPE/ECONNRESET/EINVAL on macOS
/// instead of the response; 100 B and 64 KiB bodies were fine, but always drain.
fn read_request(stream: &TcpStream) -> (String, Vec<(String, String)>, Vec<u8>) {
    let mut r = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() { break; }
        let (k, v) = h.split_once(':').unwrap();
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let len = headers.iter().find(|(k, _)| k == "content-length").map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    (line.trim_end().to_string(), headers, body)
}

/// One scripted (status, body) per connection, then close. Returns base URL and request lines.
fn serve(replies: Vec<(u16, &'static str)>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for (status, body) in replies {
            let (mut s, _) = listener.accept().unwrap();
            let (line, _headers, _body) = read_request(&s);
            tx.send(line).unwrap();
            let head = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(body.as_bytes()).unwrap();
        }
    });
    (base, rx)
}

#[test]
fn retries_then_succeeds() {
    let (base, rx) = serve(vec![(503, "busy"), (429, "slow"), (200, "[]")]);
    let a = make_test_agent();
    let mut attempts = 0;
    let body = loop {
        attempts += 1;
        match enumerate_page(&a, &base, "NC_000021.9", 100, 200, 50000, 0) {
            Outcome::Ok(b) => break b,
            Outcome::Retry(_) if attempts < 6 => continue,
            Outcome::Retry(m) | Outcome::Fail(m) => panic!("{m}"),
        }
    };
    assert_eq!((attempts, body.as_str()), (3, "[]"));
    assert_eq!(rx.recv().unwrap(),
        "GET /alleles?refseq=NC_000021.9&begin=100&end=200&fields=none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome&limit=50000&skip=0 HTTP/1.1");
}

#[test]
fn other_4xx_fails() {
    let (base, _rx) = serve(vec![(400, "{\"errorType\":\"x\"}")]);
    assert!(matches!(lookup(&make_test_agent(), &base, "x".into()), Outcome::Fail(m) if m.starts_with("HTTP 400")));
}

// Timeout test pattern: the server accepts and sleeps 3 s; agent timeout_global 500 ms ->
// Err(ureq::Error::Timeout(ureq::Timeout::Global)). For the stalled-body case, send headers
// with Content-Length: 1000 plus 2 bytes, then sleep: read_to_string() returns the same error.
// TLS-failure pattern: point an https:// URL at a plain TcpListener that answers with HTTP text:
// Err(ureq::Error::Io(e)) with e.kind() == InvalidData, and is_retryable(&err) must be false.
```

#### Gzip and bgzip reading with flate2 1.1.10 (MultiGzDecoder required for BGZF: GzDecoder read 45 of 4,599,371 ClinVar lines; re-run 1.88 s release)

```
use std::fs::File;
use std::io::{BufRead, BufReader};
use flate2::bufread::MultiGzDecoder; // read::MultiGzDecoder also works but adds its own 32 KiB buffer

fn open_gz_lines(path: &std::path::Path) -> std::io::Result<impl BufRead> {
    let f = File::open(path)?;
    // MultiGzDecoder reads every member: BGZF (ClinVar, gnomAD) and plain single-member gzip (UCSC).
    let gz = MultiGzDecoder::new(BufReader::with_capacity(1 << 20, f));
    Ok(BufReader::with_capacity(1 << 20, gz))
}
```


## nf-schema and nf-test

### Findings

- **Versions: nf-schema** (verified): CONFIRMED. Recommend nf-schema 3.0.0, published on GitHub 2026-09-29T13:58Z (not a prerelease); 25 days old on 2026-10-24. The Nextflow plugin registry lists 3.0.0 as PUBLISHED with requires '>=26.04.0'. 2.8.0 (2026-08-10) also requires >=26.04.0, so falling back to 2.x gains no older-Nextflow support. The 2.7.x line is the last that accepts '>=25.10.0'; its latest is 2.7.3 (2026-07-07). build.gradle at the tag sets version = '3.0.0' and nextflowPlugin.nextflowVersion = '26.04.0'. Pin it exactly with plugins { id 'nf-schema@3.0.0' }. Nextflow has also accepted '~3.0.0' (latest 3.0.x) since 25.04.0. Pinning in any form suppresses the plugin's 'Please pin the nf-schema version' warning; ValidationObserver warns only when the plugins list contains the bare 'nf-schema'.  
  Source: https://api.github.com/repos/nextflow-io/nf-schema/releases (3.0.0 published_at 2026-09-29T13:58:33Z, prerelease=false); https://github.com/nextflow-io/nf-schema/releases/tag/3.0.0 ; https://registry.nextflow.io/api/v1/plugins/nf-schema (3.0.0 date 2026-09-29T13:58:24Z, requires >=26.04.0, PUBLISHED; 2.8.0 >=26.04.0; 2.7.3 >=25.10.0) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/build.gradle ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/ValidationObserver.groovy ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/plugins/using-plugins.md
- **Versions: Nextflow floor implied by nf-schema** (verified): CONFIRMED. The newest stable Nextflow released on or before 2026-10-10 is v26.04.7: GitHub release published 2026-10-08T16:28:35Z, not a prerelease. Bioconda nextflow-26.04.7-h2a3209d_0.conda was uploaded 2026-10-08 23:52 UTC. Too new under the two-week rule: v26.10.0 (2026-10-12), 26.10.1/26.10.2 (2026-10-15), 26.10.3 (2026-10-20) and 26.04.9 (2026-10-22). 26.04.8 (2026-10-20) is marked prerelease. v26.09.2-edge (2026-10-06) is an edge prerelease. Use manifest.nextflowVersion = '!>=26.04.7'. The docs say the '!' prefix stops execution when the running version does not match; without it Nextflow only warns.  
  Source: https://api.github.com/repos/nextflow-io/nextflow/releases ; https://github.com/nextflow-io/nextflow/releases/tag/v26.04.7 ; https://api.anaconda.org/package/bioconda/nextflow/files ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/config.md (manifest.nextflowVersion)
- **Versions: nf-test** (verified): CONFIRMED. Recommend nf-test 0.9.5: GitHub release v0.9.5 published 2026-03-19T15:42Z, and it is the latest release. Bioconda nf-test-0.9.5-h2a3209d_0.conda was uploaded 2026-03-19 16:50 UTC. The v0.9.4 release (2026-02-12) notes include 'Strict syntax support' (PR #336). The v0.9.5 notes include 'Implementation of topics channel testing for processes and workflows' (PR #355). The only release asset is nf-test-0.9.5.tar.gz.  
  Source: https://api.github.com/repos/askimed/nf-test/releases ; https://github.com/askimed/nf-test/releases/tag/v0.9.5 ; https://github.com/askimed/nf-test/releases/tag/v0.9.4 ; https://api.anaconda.org/package/bioconda/nf-test/files
- **Strict parser is the default in 26.04 and changes CLI param types** (verified): CONFIRMED. NF.getSyntaxParserVersion() returns SysEnv NXF_SYNTAX_PARSER, default 'v2'; NXF_SYNTAX_PARSER=v1 restores the old parser. CmdRun.parseParamValue returns the raw string when the parser is v2, or when NXF_DISABLE_PARAMS_TYPE_DETECTION is set. Launcher rewrites a bare '--x' to '--x=true'. So --pad 30 gives '30', --allow_mismatches gives 'true', and --allow_mismatches false gives the String 'false', which is truthy; strict-syntax.md shows this exact example. Values read from -params-file (JSON/YAML) and from config keep their types. Normalise CLI values in main.nf (e.g. .toString().toBoolean(), .toString().toInteger()) or use a typed params {} block. Other strict-syntax rules: all statements, including paramsHelp()/validateParameters(), must be inside the entry workflow; only hard casts ('as') are allowed; workflow.onComplete must be assigned inside the entry workflow, or written as an onComplete: section (since 25.10).  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/NF.groovy (lines 31-37) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/cli/CmdRun.groovy (parseParamValue, lines 789-803) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/cli/Launcher.groovy (lines 341-345) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/strict-syntax.md ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/migrations/26-04.md
- **-params-file values count as 'CLI params' for nf-schema casting (affects nf-test)** (verified): NEW. In CmdRun, cliParams = parsedParams(...) (line 341). parsedParams reads the -params-file first and then the CLI params into one map, and that map becomes session.cliParams. nf-schema's cast_cli_params uses session.cliParams. String values from a params file are therefore cast too. nf-test passes `when { params { chrom = "21" } }` through a -params-file, so chrom is validated as Integer 21, exactly like `--chrom 21`. This is a second reason to use "type": ["string","integer"] for chrom. Pipeline-visible params are not changed: the cast happens only in the copy that is validated.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/cli/CmdRun.groovy (line 341; parsedParams lines 720-743) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/Session.groovy (init: this.cliParams = cliParams) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/parameters/ParameterValidator.groovy (lines 145-157)
- **Typed params block vs legacy params (design choice)** (verified): CONFIRMED, with one correction. The typed params { name: Type = default } block exists since 25.10.0. Nullable/optional params use a '?' suffix (e.g. 'input: Path?'); the compiler sets ParamsDsl.declare(optional=true) when the type carries the NULLABLE marker. Since 26.04.0, a Boolean with no default defaults to false. CLI values are cast by the declared type. ParamsDef.apply(session) runs in BaseScript just before the entry workflow is invoked, so a required param (no default, no '?') fails the run before any --help code can execute. CORRECTION: ParamsDsl.apply throws 'Parameter `x` was specified on the command line or params file but is not declared in the script or config' only when the key is neither declared in the params block nor present in config params; keys defined in nextflow.config params are accepted. nf-schema 3.0.0's example pipelines all use legacy params in nextflow.config. That remains the simplest path for --help and nf-test, at the cost of normalising strings by hand.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/script/ParamsDsl.groovy (lines 49-90) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nf-lang/src/main/java/nextflow/script/control/ScriptToGroovyVisitor.java (visitParams) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/script/BaseScript.groovy (paramsDef.apply before entryFlow.invoke_a) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/workflow-typed.md ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/test/groovy/nextflow/script/ParamsDslTest.groovy ; https://github.com/nextflow-io/nf-schema/tree/3.0.0/examples
- **nf-schema: includes and function signatures in 3.0.0** (verified): CONFIRMED. Include line: include { validateParameters; paramsHelp; paramsSummaryLog } from 'plugin/nf-schema'.
- validateParameters(Map options=[:]) reads parameters_schema and cast_cli_params.
- paramsHelp(Map options=[:]) reads parameter, beforeText, afterText, command, showHidden, fullHelp and parameters_schema.
- paramsSummaryLog(Map options=[:]) reads parameters_schema, beforeText and afterText. It does not read 'monochrome_logs', although 2.7.0 added that; colours follow validation.monochromeLogs.
Breaking changes from 2.x: paramsSummaryLog(workflow) becomes paramsSummaryLog(), and paramsHelp('x') becomes paramsHelp(parameter: 'x'). docs/parameters/help_text.md names the schema option 'parametersSchema', but the code reads 'parameters_schema'. Keeping the default nextflow_schema.json avoids the issue.  
  Source: https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/ValidationExtension.groovy (lines 87-97, 141-189, 207-213) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/parameters/ParameterValidator.groovy (lines 128-134) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/docs/parameters/help_text.md ; https://nextflow-io.github.io/nf-schema/3.0.0/3_0_0_migration_guide/
- **nf-schema: how --help, --helpFull and --showHidden work in 3.0.0** (verified): CORRECTED. Automatic help was removed in 3.0.0; the migration guide says validation.help.enabled 'does no longer serve any purpose'. HelpConfig has no 'enabled' field. docs/configuration/configuration.md at the 3.0.0 tag still documents help.enabled, so that page is stale. The pipeline must test params.help, print paramsHelp(...) and stop with `exit 0`. `exit` has been deprecated since 22.06.0-edge, but nf-schema's 3.0.0 paramsHelp example uses it, and its expected log was produced on Nextflow 26.04.4.
CORRECTION: paramsHelp does NOT read --helpFull or --showHidden from params by itself. Full help comes only from the fullHelp option. showHidden is set from the showHidden option (default false), and that overrides validation.help.showHidden from config. HelpConfig takes params.showHidden only when it is a Boolean, which a CLI '--showHidden' is not under parser v2. Both options are passed through `as Boolean`, and the String 'false' as Boolean is true. The pipeline must therefore pass real Booleans: paramsHelp(parameter: ..., fullHelp: params.helpFull.toString()=='true', showHidden: params.showHidden.toString()=='true').
A bare --help arrives as the String 'true', so the example uses ['true','false'].contains(params.help) ? null : params.help. Passing parameter:'true' throws 'Unable to create help message: Specified param 'true' does not exist in JSON schema.' The help output can be customised with validation.help.{command, beforeText, afterText, enumLength, shortParameter, fullParameter, showHiddenParameter}; the paramsHelp options beforeText, afterText and command override the config values.  
  Source: https://raw.githubusercontent.com/nextflow-io/nf-schema/3.0.0/docs/3_0_0_migration_guide.md ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/ValidationExtension.groovy (paramsHelp, lines 141-189) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/config/HelpConfig.groovy (lines 24-58, 76-86) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/help/HelpMessageCreator.groovy (line 65) ; https://github.com/nextflow-io/nf-schema/tree/3.0.0/examples/paramsHelp (main.nf, log.txt shows 'version 26.04.4') ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/docs/configuration/configuration.md (still lists help.enabled) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/stdlib-namespaces.md (exit deprecated 22.06.0-edge)
- **nf-schema: schema structure** (verified): CONFIRMED.
- '$schema' must be exactly 'https://json-schema.org/draft/2020-12/schema'; JsonSchemaValidator logs an error and throws SchemaValidationException on anything else.
- Group params under '$defs'. Each group has title, type:object, its own 'required' and 'properties', and every group needs an entry in top-level 'allOf': [{"$ref": "#/$defs/<group>"}]. Ungrouped params go in top-level 'properties'.
- Required params: list them in the group's 'required' array and set their config default to null. ParameterValidator serialises params with excludeNulls(), so a null param is absent and 'required' fires.
- 'enum' works for strings and numbers, and help prints '(accepted: ...)'.
- 'pattern' is evaluated by dev.harrel json-schema 1.9.1 PatternEvaluator as Pattern.compile(p).matcher(s).find(). It is an unanchored Java regex that skips non-string values, so anchor it explicitly.
- Other documented keys: errorMessage, deprecated, fa_icon, hidden, help_text, minLength/maxLength, format (file-path, directory-path, path, file-path-pattern), exists, mimetype, schema, minimum/maximum, uniqueItems, uniqueEntries.
- The spec lists 'null' as an unsupported type, and object/array only for file/nested validation.  
  Source: https://github.com/nextflow-io/nf-schema/blob/3.0.0/docs/nextflow_schema/nextflow_schema_specification.md ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/validators/JsonSchemaValidator.groovy (lines 48-59) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/parameters/ParameterValidator.groovy (excludeNulls, line 139) ; https://github.com/harrel56/json-schema/blob/1.9.1/lib/src/main/java/dev/harrel/jsonschema/Evaluators.java (PatternEvaluator) ; nf-schema 3.0.0 build.gradle pins dev.harrel:json-schema:1.9.1
- **nf-schema gotcha: numeric chromosome values fail a plain string type** (verified): CONFIRMED and widened. Under parser v2, validateParameters defaults to cast_cli_params = isSyntaxParserV2() = true. It casts String values whose keys are in session.cliParams with Types.parseParamValue: 'true'/'false' become Boolean, digit strings become Integer/Long/Double, everything else stays a String. The cast applies only to the JSON copy being validated; session.params is untouched. session.cliParams also contains -params-file entries (see the separate finding), so nf-test params are cast as well. Results: `--chrom 21` (or chrom = "21" in an nf-test params block) is validated as Integer 21 and fails "type": "string" with 'Value is [integer] but should be [string]'. `--gnomad_version 4.1` becomes Double 4.1; '4.1.1' stays a String. validation.lenientMode=true makes LenientTypeEvaluator accept any scalar for a 'string' type, but then 'pattern' no longer applies (string-only), so `--chrom 99` passes. Fix: "type": ["string","integer"] plus an anchored pattern (strings) and minimum 1 / maximum 22 (integers); likewise ["string","number"] for gnomad_version. Type arrays work in the default (non-lenient) validator. nf-schema's own test schema declares help as ["string","boolean"], and its built-in help param uses ['boolean','string'].  
  Source: https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/parameters/ParameterValidator.groovy (lines 131-157) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/utils/Types.groovy (parseParamValue, lines 90-104) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/validators/evaluators/LenientTypeEvaluator.groovy ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/help/HelpMessageCreator.groovy (line 282) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/testResources/nextflow_schema.json ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/test/groovy/nextflow/validation/ValidateParametersTest.groovy ('* --outdir (10): Value is [integer] but should be [string]')
- **nf-schema: unrecognised params and the help params** (verified): CORRECTED: the help params never trip the unrecognised check. Params not described in the schema produce a WARN by default. validation.logging.unrecognisedParams accepts skip/debug/info/warn/error; with 'error', ValidationLogger throws SchemaValidationException. failUnrecognisedParams/failUnrecognisedHeaders were removed in 3.0.0. ignoreParams always contains 'nf_test_output', and validation.ignoreParams and validation.defaultIgnoreParams (lists) add to it.
CORRECTION: the ValidationConfig constructor always runs `ignoreParams += [help.shortParameter, help.fullParameter, help.showHiddenParameter]` (lines 200-201). help/helpFull/showHidden (or their renamed versions) are therefore exempt even when the schema omits them, so no defaultIgnoreParams entry is needed. Two pieces of evidence, both read rather than run: the test 'should fail for unexpected param' (unrecognisedParams='error'; helpFull/showHidden are injected but missing from the schema) expects only '* --xyz'; and examples/validationFailUnrecognisedParams (schema without any help params) expects only '* --foo: bar'. Consequence for the plan: with 'error', every other key in nextflow.config params, on the CLI or in an nf-test params block must be declared in nextflow_schema.json, or the run fails. The original minimal schema lacked genome and clinvar_vcf, which the pipeline test snippet passes.  
  Source: https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/config/ValidationConfig.groovy (lines 61-64, 155-175, 200-201) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/config/LoggingConfig.groovy ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/logging/ValidationLogger.groovy ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/main/groovy/nextflow/validation/parameters/ParameterValidator.groovy (lines 173-208) ; https://github.com/nextflow-io/nf-schema/blob/3.0.0/src/test/groovy/nextflow/validation/ValidateParametersTest.groovy (line 640) ; https://github.com/nextflow-io/nf-schema/tree/3.0.0/examples/validationFailUnrecognisedParams
- **nf-test install** (verified): CONFIRMED, with one addition. `curl -fsSL https://get.nf-test.com | bash -s 0.9.5` downloads https://github.com/askimed/nf-test/releases/download/v0.9.5/nf-test-0.9.5.tar.gz and extracts it into the current directory, which gives the `nf-test` launcher. ADDITION: it also moves nf-test.jar to $HOME/.nf-test/nf-test.jar. Alternatives: the GitHub release asset, or conda bioconda::nf-test=0.9.5. nf-test finds Nextflow via NEXTFLOW_HOME, then PATH, then /usr/local/bin. `nf-test init` writes nf-test.config (testsDir 'tests', workDir '.nf-test', configFile 'tests/nextflow.config', profile '') and, if absent, a tests/nextflow.config containing only a comment header. Add .nf-test/ to .gitignore.  
  Source: https://get.nf-test.com (script read, not run) ; https://www.nf-test.com/installation/ ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/resources/com/askimed/nf/test/commands/init/nf-test.config ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/resources/com/askimed/nf/test/commands/init/nextflow.config ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/commands/InitCommand.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/nextflow/NextflowCommand.java (BinaryFinder)
- **nf-test: where to run it and how configs are layered** (verified): CONFIRMED. Run nf-test from the directory that holds nf-test.config: it loads 'nf-test.config' relative to its working directory. That must be pipeline/, next to nextflow.config, because every test adds new File("nextflow.config") from that working directory, and process/workflow mock scripts are written there as .nf-test-<hash>.nf. The command is:
nextflow [-quiet unless --verbose] -log meta/nextflow.log run <script> -c ./nextflow.config -c <configFile from nf-test.config> -c <suite config> -c <test config> -params-file meta/params.json -ansi-log false [-profile a,b] [-with-trace meta/trace.csv] -w <launchDir>/work <options>
Nextflow runs with cwd = .nf-test/tests/<testhash>. Later -c files override earlier ones and the project config. Note the project nextflow.config is loaded twice: as the project config and again as the first -c. Suite/test `config` and suite `script` paths that start with ./ or ../ resolve against the test file's directory; other relative paths resolve against nf-test's working directory. `nf-test test -c <file>` / `--config <file>` loads a different nf-test.config, which can set its own configFile, profile and options. Config.getConfigFile() does not check existence, so keep tests/nextflow.config present.  
  Source: https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/nextflow/NextflowCommand.java (execute, lines 162-211) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/process/ProcessTest.java (lines 170-240) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/pipeline/PipelineTest.java (lines 85-126) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/AbstractTest.java (lines 84-111) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/AbstractTestSuite.java (makeAbsolute/isRelative) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/commands/RunTestsCommand.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/config/Config.java (getConfigFile) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/config.md
- **nf-test: profiles** (verified): CORRECTED (empty-profile detail). Profiles are applied in order: nf-test.config `profile`, then suite-level `profile "x"`, then CLI `nf-test test --profile x`. NextflowCommand.addProfile clears the earlier profiles unless the new value starts with '+'. Comma lists are allowed. Example: with 'host' in config, `--profile local` gives -profile local, and `--profile +slurm` gives -profile host,slurm. CORRECTION: a blank profile is ignored only in nf-test.config, where Config.getProfile() returns null for blank values. A suite-level `profile ""` or CLI `--profile ""` CLEARS the earlier profiles and passes `-profile ''`, which Nextflow's ConfigBuilder turns into its default profile ('standard'). The docs table claims the default nf-test.config profile is "docker"; the source default is null.  
  Source: https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/nextflow/NextflowCommand.java (addProfile, lines 66-77) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/config/Config.java (profile = null; getProfile blank -> null) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/TestSuiteBuilder.java (configure before suite closure) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/TestExecutionEngine.java (CLI profiles added last) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/config/ConfigBuilder.groovy (line 158: profile = value ?: DEFAULT_PROFILE) ; https://www.nf-test.com/docs/configuration/
- **nf-test: options, stub, tags, params** (verified): CONFIRMED, with refinements. `options "-stub"` can be set in nf-test.config, per suite or per test. Each level replaces the one above: a test copies the suite's options when it is created, and a test-level `options ""` removes a suite-level -stub. Nextflow accepts both -stub and -stub-run (CmdRun @Parameter names=['-stub-run','-stub']). Tags work at suite and test level and are selected with `nf-test test --tag a,b`. REFINEMENT: matching is case-insensitive OR logic. A test matches if a requested tag equals one of its tags or its name, or (recursively) its suite's tags or name. A suite-level tag therefore selects every test in that suite. `when { params { ... } }` values are written to meta/params.json and passed as -params-file, which overrides config params; `params { load("$baseDir/tests/params.json") }` (or .yaml) is supported. Process/workflow/function tests also add nf_test_output to params; pipeline tests do not. Global variables: baseDir/projectDir, moduleDir, moduleTestDir, launchDir (.nf-test/tests/<hash>), metaDir, workDir, outputDir. REFINEMENT: baseDir/projectDir is nf-test's working directory (System user.dir), which equals the nf-test.config directory only when nf-test runs from there. Use outdir = "$outputDir" for pipeline tests.  
  Source: https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/AbstractTest.java (lines 53-55, 79-82, 191-197) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/AbstractTestSuite.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/TagQuery.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/commands/RunTestsCommand.java (--tag split=",") ; https://www.nf-test.com/docs/testcases/params/ ; https://www.nf-test.com/docs/testcases/global_variables/ ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/cli/CmdRun.groovy (line 269)
- **nf-test: test types and assertions** (verified): CONFIRMED. Test types:
- nextflow_process { script; process } tests one process.
- nextflow_workflow { script; workflow "NAME" } tests a named sub-workflow, with inputs given as input[N].
- nextflow_pipeline { script "main.nf" } runs the whole entry workflow; use it for the workflow stub and real tests.
Assertions:
- process.success/failed/exitStatus/errorReport/stdout and process.trace.tasks().
- process.out.<emit> or process.out[0].
- workflow.success/failed/exitStatus/stdout and workflow.trace.succeeded()/failed()/tasks().
- snapshot(obj, ...).match() / .match("name") and snapshot(...).md5().
- path(x).md5 / .linesGzip / .json / .exists(), and path(dir).list(); exists() and list() are nf-test PathExtension methods.
- assertAll(...) and with(){}.
autoSort defaults to true, so output channels are sorted.  
  Source: https://www.nf-test.com/docs/testcases/nextflow_process/ ; https://www.nf-test.com/docs/testcases/nextflow_workflow/ ; https://www.nf-test.com/docs/testcases/nextflow_pipeline/ ; https://www.nf-test.com/docs/assertions/snapshots/ ; https://github.com/askimed/nf-test/blob/v0.9.5/docs/docs/assertions/files.md ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/extensions/PathExtension.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/extensions/Snapshot.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/config/Config.java (autoSort = true)
- **nf-test: snapshots** (verified): CONFIRMED, with a corrected reason. Snapshots live next to the test file as <testfile>.snap (SnapshotFile.createFilename = suite filename + '.snap'). Commit them. `--update-snapshot` re-records snapshots that fail; `--ci` throws 'CI mode activated and snapshot with id ... not found' instead of writing new ones; `--clean-snapshot` removes obsolete entries. Paths are serialised as 'name:md5,<hash>'. For names ending in .gz, FileUtil.getMd5 hashes the gunzipped content. If hashing throws, PathConverter prints a stack trace and stores the raw value, i.e. the absolute path. CORRECTED REASON: that path differs between runs because the task work directory hash includes the session UUID (TaskHasher adds session.uniqueId). The per-test directory is stable (md5 of suite filename + test name). Consequence unchanged: stubs must write valid gzip (`printf '' | gzip -c > x.tsv.gz`), not `touch x.tsv.gz`. Content that varies by date, such as manifest.json source labels, should not be snapshotted.  
  Source: https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/extensions/SnapshotFile.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/extensions/Snapshot.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/util/FileUtil.java (getMd5, lines 97-115) ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/lang/extensions/util/PathConverter.java ; https://github.com/askimed/nf-test/blob/v0.9.5/src/main/java/com/askimed/nf/test/core/AbstractTest.java (getHash) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskHasher.groovy (line 53)
- **Nextflow stub: semantics** (verified): CONFIRMED. Under -stub / -stub-run, TaskRun.resolve calls resolveStub() only when the process has a stub block; otherwise it runs the real script, as the docs also say. Only the script body is swapped. Executor, container, labels, resources, maxForks, output declarations and publishDir are untouched; this comes from the code, not stated in the docs. TaskHasher adds a 'stub-run' key when stubRun is on and the process has a stub, so stub results are not reused by a real -resume. Practical consequence: stub tests under a Docker profile still start the container. Run them with the container-free `host` profile, or a test config that disables containers.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/process.md (Stub) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskRun.groovy (resolve, lines 831-835) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskHasher.groovy (lines 123-125)
- **maxForks** (verified): CONFIRMED. maxForks is the maximum number of concurrent task executions of one process; there is no limit by default, though local CPUs and executor.queueSize still cap concurrency. With maxForks 1 on both ENUMERATE and LOOKUP, at most two Registry streams run, which matches the spec. process.md lists executor, label, maxForks and secret as the directives that cannot be dynamic. It can be set in the process or in config, e.g. withName: 'ENUMERATE|LOOKUP' { maxForks = 1 }.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/process.md (maxForks) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/process.md (Dynamic directives)
- **errorStrategy with backoff** (verified): CONFIRMED.
- The docs example is errorStrategy { sleep(Math.pow(2, task.attempt) * 200 as long); return 'retry' } with maxRetries 5. With 1000 instead of 200, the waits are 2 s, 4 s, 8 s...
- 'as' binds looser than '*' in the strict grammar too: relationalCastExprAlt comes after multDivExprAlt in ScriptParser.g4, so the whole product is cast.
- In checkErrorStrategy, task.attempt is the attempt that just failed (1 on the first failure); the retry gets attempt = taskErrCount + 1.
- Defaults: errorStrategy TERMINATE, maxRetries 1, maxErrors -1 (unlimited).
- The strategy closure runs in the launcher inside TaskProcessor.resumeOrDie, which is `synchronized` on the processor, so a long sleep blocks that process's failure handling. Keep it short and let the Rust client do the long backoff.
- sleep(ms) is a global stdlib function.
- In config files, dynamic values must be closures (withLabel: network { errorStrategy = { ... }; maxRetries = 2 }), and the strict config parser allows statements only inside closures. NEW: a closure assigned to a config option gets ScriptDsl, plus ProcessDsl in the process scope, pushed for name resolution, so sleep() inside it passes the strict config check (VariableScopeVisitor.visitConfigAssign).  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/process.md (Dynamic retry with backoff) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/process.md (errorStrategy, maxRetries, maxErrors) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskProcessor.groovy (resumeOrDie line 1039, checkErrorStrategy lines 1155-1193) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/script/ProcessConfig.groovy (lines 45-47) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nf-lang/src/main/antlr/ScriptParser.g4 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nf-lang/src/main/java/nextflow/config/control/VariableScopeVisitor.java (lines 114-140, 284-295) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/strict-syntax.md (Configuration syntax)
- **publishDir modes** (verified): CONFIRMED.
- Modes: symlink (the default; absolute link), rellink, link (hard link), copy, copyNoFollow, and move (terminal processes only). If no mode is set, the default becomes rellink when stageInMode is 'rellink'.
- Only files matching the output declarations are published. Publishing is asynchronous, so downstream steps must use channels, not the publish directory.
- overwrite defaults to true, or false on -resume. failOnError has defaulted to true since 24.04.0.
- Other options: pattern, saveAs, enabled, and the S3-only contentType/storageClass/tags.
- When the target filesystem differs from the source, is not the default filesystem (e.g. s3://), or is under /fusion/s3/, link modes switch to copy with a one-time warning (warn1 firstOnly).
- Use mode: 'copy' for FINALIZE/REPORT so results survive deleting work/.
- Workflow outputs are the newer alternative. publishDir is not marked deprecated in the 26.04.7 docs.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/process.md (publishDir) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/PublishDir.groovy (validatePublishMode, lines 566-582)
- **label / withLabel / withName** (verified): CONFIRMED.
- `label 'x'` can be repeated. Names may contain alphanumerics or '_' and must start and end with a letter; network, sort, genome, build and small all qualify. label cannot be dynamic.
- Selectors accept regexes ('a|b') and negation ('!a').
- Selector priority, lowest to highest: process settings without a selector; directives in the process definition; withLabel; withName on the process name; withName on the include alias; withName on the fully qualified name ('WF:PROC'). A test config that overrides a withLabel setting must therefore use withLabel or withName itself.
- Config file precedence, lowest to highest: $NXF_HOME/config, project nextflow.config, launch-dir nextflow.config, -c files in order; -C replaces them all.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/process.md (label) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/process.md (Dynamic directives exclusions) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/config.md (Configuration files; Selector expressions; Selector priority)
- **Later -c file overrides a profile setting from an earlier file (resolves an open question)** (verified): NEW, from code. ConfigBuilder.merge0 parses each config file separately with the selected profiles applied (parser.setProfiles), then calls result.merge(config) in file order. So a top-level `docker.enabled = false` in a later -c file (e.g. tests/nextflow.config passed by nf-test) overrides docker.enabled = true set by a profile block in an earlier file (the project nextflow.config). Not run.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/config/ConfigBuilder.groovy (buildConfig0 loop and merge0, lines 390-445)
- **bin/ on PATH** (verified): CONFIRMED, with refinements. Session.setBaseDir registers <projectDir>/bin when it exists and is a directory; if it doesn't exist, nothing happens. With main.nf in pipeline/, that is pipeline/bin. For a local work dir, TaskProcessor appends the bin dir AFTER the existing PATH ('$PATH:<bin>'), so system commands with the same name win. Container builders mount the bin dirs (BashWrapperBuilder addMounts(binDirs)). REFINEMENT on caching: TaskHasher adds only the bin scripts whose names appear as tokens in the task script, so editing a bin script re-runs only the tasks that invoke it. On AWS Batch, a non-empty bin/ is uploaded to the S3 work dir unless executor.disableRemoteBinDir = true (read in Session.groovy line 466; not listed in docs/reference/config.md).  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/sharing.md (The bin directory) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/Session.groovy (setBaseDir lines 628-639; line 466) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskProcessor.groovy (getBinDirs, getProcessEnvironment) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskHasher.groovy (getTaskBinEntries) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/BashWrapperBuilder.groovy (line 707) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/plugins/nf-amazon/src/main/nextflow/cloud/aws/batch/AwsBatchExecutor.groovy (uploadBinDir)
- **splitText / splitCsv deprecated (affects the spec's batching)** (likely): CONFIRMED, except for the runtime-warning claim, which is now unverified. operator.md marks splitCsv, splitText, splitJson, splitFasta and splitFastq '{deprecated} 26.04.0: Use flatMap with the Path <method> method instead', e.g. ch.flatMap { f -> f.splitText(by: 500) }. stdlib-types.md says the Path methods take the operators' options. nf-lang's Channel type marks these operators @Deprecated, which the language server and `nextflow lint` use. NOT VERIFIED: whether `nextflow run` prints a deprecation warning at runtime; I found no runtime warning in the operator implementation.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/operator.md ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/stdlib-types.md (Path splitCsv/splitText) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nf-lang/src/main/java/nextflow/script/types/Channel.java (lines 183-186)
- **Plugin download and offline use** (verified): CONFIRMED. Nextflow downloads declared plugins on first use, including runs launched by nf-test (they are ordinary `nextflow run` calls), and caches them in NXF_PLUGINS_DIR (default $HOME/.nextflow/plugins). For offline nodes, pre-fetch with `nextflow plugin install nf-schema@3.0.0`, then copy the nextflow binary and $HOME/.nextflow. Versions are required offline; '~' ranges resolve against the local cache. Plugin declarations in config files are ignored when -plugins is given on the CLI.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/plugins/using-plugins.md
- **What was not executed** (verified): Nextflow, nf-test and Docker are not on PATH here (`command -v` found none; /usr/bin/java is the macOS stub), and nothing was installed. No Nextflow or nf-test snippet was run. All findings come from reading release metadata, docs and source at the cited tags: nf-schema 3.0.0, nf-test v0.9.5, Nextflow v26.04.7, dev.harrel json-schema 1.9.1. I downloaded these fresh into /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/research/recheck-nfschema-nftest/ (nfschema-3.0.0/, nftest-0.9.5/, nextflow-26.04.7/, harrel-1.9.1/, getnftest/install.sh, plus the release/registry/bioconda JSON). The corrected schema snippet is at /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/research/recheck-nfschema-nftest/snippet-check/nextflow_schema.json. It was checked only as valid JSON, and its patterns with Python re.search, which has the same unanchored-search semantics as Java's find() for these patterns. It was not run through nf-schema.  
  Source: local: `command -v nextflow nf-test docker java` ; python3 json/re checks

### Open questions the researchers raised

- Typed `params {}` block or legacy params in nextflow.config? Legacy params work with nf-schema --help and nf-test, but main.nf must convert CLI strings to types under parser v2. A typed block casts CLI values, but it rejects CLI or params-file keys that are declared neither in the block nor in config. It also checks required typed params before the entry workflow runs, so a missing required param fails before --help can print.
- Should --chrom accept bare 'M'? The example pattern accepts chrM, MT and chrMT, and rejects M.
- Pin nf-schema exactly ('nf-schema@3.0.0') or allow patches ('~3.0.0')? The spec asks for a pin; exact is the reproducible choice.
- None of the snippets were executed (no Nextflow, nf-test or Docker here, and installing was not allowed). The first implementation task should run `nextflow run pipeline --help`, `--helpFull`, a validateParameters failure (e.g. --chrom 23), an unrecognised param (--foo) with unrecognisedParams='error', and one nf-test stub test.
- Does `nextflow run` 26.04.7 print a runtime deprecation warning for splitText/splitCsv, or only `nextflow lint` / the language server? Not verified. Either way the plan can use flatMap { f -> f.splitText(by: N) } now.
- With unrecognisedParams = 'error', every param in the spec table (merge_gap, max_window, flank, cache, windows_per_task, batch_size, container, slurm_queue, network_queue, aws_queue, aws_region, ...) must be in nextflow_schema.json. Should container/queue/region params instead go in the hidden group, or be handled via validation.ignoreParams?
- outdir uses format 'directory-path'. For an s3:// outdir, nf-schema will probably query S3 during validation (an existence/type check), so the launcher needs credentials at that point. Not verified against FormatDirectoryPathEvaluator for S3 paths.

### Snippets

#### pipeline/nextflow.config: plugin pin, validation scope, legacy params (excerpt) [FIXED: dropped the unneeded defaultIgnoreParams; declared helpFull/showHidden; added params the tests pass]

```
manifest {
    name            = 'clingen-index-regeneration'
    nextflowVersion = '!>=26.04.7'      // '!' = stop (not just warn) if the running Nextflow does not match
}

plugins {
    id 'nf-schema@3.0.0'                // 3.0.0 requires Nextflow >=26.04.0
}

validation {
    logging {
        unrecognisedParams = 'error'    // replaces the removed failUnrecognisedParams
    }
    // help/helpFull/showHidden are always in ignoreParams (ValidationConfig.groovy l.200-201): no defaultIgnoreParams needed
    help {
        command = 'nextflow run pipeline -profile local --chrom 21 --outdir results/chr21'
    }
    // NOTE: validation.help.enabled does nothing in 3.x - call paramsHelp() yourself
}

params {
    chrom            = null      // required in schema; null so 'required' fires
    regions          = 'pc_exons'
    pad              = 20
    datasets         = 'clinvar'
    gnomad_version   = 'latest'
    gnomad_mirror    = 'gcs'
    clinvar_vcf      = 'latest'
    genome           = null      // null = build from UCSC hg38.fa.gz
    outdir           = 'results'
    allow_mismatches = false
    help             = false
    helpFull         = false     // declared so main.nf can read them before validateParameters()
    showHidden       = false     //   (reading an undeclared param logs 'Access to undefined parameter')
    // With unrecognisedParams = 'error', every other param here (merge_gap, max_window, flank, cache,
    // windows_per_task, batch_size, container, slurm_queue, network_queue, aws_queue, aws_region)
    // must also be declared in nextflow_schema.json, or validation fails.
}
```

#### pipeline/nextflow_schema.json: draft 2020-12, $defs + allOf, required/enum/pattern [FIXED: added gnomad_version (string|number), clinvar_vcf and genome, which the tests pass; copy at scratchpad/research/recheck-nfschema-nftest/snippet-check/nextflow_schema.json; JSON-valid, not run through nf-schema]

```
{
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "https://example.invalid/clingen/pipeline/nextflow_schema.json",
    "title": "clingen index regeneration pipeline parameters",
    "description": "Regenerates the ClinGen CA-ID index (caid.fst + manifest.json).",
    "type": "object",
    "$defs": {
        "input_options": {
            "title": "Input options",
            "type": "object",
            "description": "What to regenerate.",
            "required": ["chrom"],
            "properties": {
                "chrom": {
                    "type": ["string", "integer"],
                    "description": "Chromosome: 1-22, X, Y, MT (optional chr prefix) or all.",
                    "pattern": "^((chr)?([1-9]|1[0-9]|2[0-2]|X|Y|MT)|chrM|all)$",
                    "minimum": 1,
                    "maximum": 22,
                    "errorMessage": "Use 1-22, X, Y, MT (a chr prefix is accepted) or all."
                },
                "regions": {
                    "type": "string",
                    "description": "Region set (cds, pc_exons, exons, genes) or a BED path.",
                    "default": "pc_exons",
                    "pattern": "^(cds|pc_exons|exons|genes|\\S+\\.bed(\\.gz)?)$"
                },
                "datasets": {
                    "type": "string",
                    "description": "Comma-separated harvested datasets.",
                    "default": "clinvar",
                    "pattern": "^(clinvar|gnomad_genomes|gnomad_exomes)(,(clinvar|gnomad_genomes|gnomad_exomes))*$"
                },
                "gnomad_version": {
                    "type": ["string", "number"],
                    "description": "gnomAD release: latest, or a pinned version such as 4.1.1.",
                    "default": "latest",
                    "pattern": "^(latest|\\d+\\.\\d+(\\.\\d+)?)$"
                },
                "gnomad_mirror": {
                    "type": "string",
                    "description": "Where to stream gnomAD from.",
                    "default": "gcs",
                    "enum": ["gcs", "aws"]
                },
                "clinvar_vcf": {
                    "type": "string",
                    "description": "latest, or a local clinvar.vcf.gz for an exact rerun.",
                    "default": "latest",
                    "pattern": "^(latest|\\S+\\.vcf\\.gz)$"
                },
                "genome": {
                    "type": "string",
                    "format": "file-path",
                    "exists": true,
                    "description": "Prebuilt vareffect genome; built from UCSC hg38.fa.gz when not given."
                },
                "pad": {
                    "type": "integer",
                    "description": "Padding added to each region (bp).",
                    "default": 20,
                    "minimum": 0
                }
            }
        },
        "output_options": {
            "title": "Output options",
            "type": "object",
            "properties": {
                "outdir": {
                    "type": "string",
                    "format": "directory-path",
                    "description": "Local path or s3:// URI for the results.",
                    "default": "results"
                },
                "allow_mismatches": {
                    "type": "boolean",
                    "description": "Publish despite validate/reach failures.",
                    "default": false
                }
            }
        },
        "generic_options": {
            "title": "Generic options",
            "type": "object",
            "properties": {
                "help": {
                    "type": ["boolean", "string"],
                    "description": "Show help; give a parameter name for its detailed help.",
                    "hidden": true
                }
            }
        }
    },
    "allOf": [
        { "$ref": "#/$defs/input_options" },
        { "$ref": "#/$defs/output_options" },
        { "$ref": "#/$defs/generic_options" }
    ]
}
```

#### pipeline/main.nf: entry workflow with help / validate / summary (strict syntax; nf-schema 3.0.0 signatures) [FIXED: wire --helpFull and --showHidden, passing real Booleans]

```
include { validateParameters; paramsHelp; paramsSummaryLog } from 'plugin/nf-schema'
include { ENUMERATE } from './modules/registry'

workflow {
    // Under the v2 parser CLI values are Strings: bare --help -> 'true', '--help chrom' -> 'chrom'.
    // From nextflow.config or a params file they keep their type (Boolean false by default).
    def help       = params.help.toString()
    def helpFull   = params.helpFull.toString() == 'true'
    def showHidden = params.showHidden.toString() == 'true'
    if (help != 'false' || helpFull) {
        log.info paramsHelp(
            parameter:  help in ['true', 'false'] ? null : help,
            fullHelp:   helpFull,     // must be a real Boolean: paramsHelp does `as Boolean`, and 'false' as Boolean is true
            showHidden: showHidden    // HelpConfig ignores a String --showHidden, so pass it explicitly
        )
        exit 0   // deprecated since 22.06.0-edge but works; nf-schema's own 3.0.0 example uses it
    }

    validateParameters()          // throws SchemaValidationException on failure
    log.info paramsSummaryLog()   // 3.x: no 'workflow' argument

    // CLI values are Strings under the strict parser (default in 26.04): normalise explicitly.
    // nf-schema's casting only affects the copy it validates, never params.*
    def chrom           = params.chrom.toString().replaceFirst(/^chr/, '')
    def pad             = params.pad.toString().toInteger()
    def allowMismatches = params.allow_mismatches.toString().toBoolean()
    def gnomadVersion   = params.gnomad_version.toString()

    // ... build channels and call processes here ...
}
```

#### Process with label, maxForks, stub (valid gzip), publishDir copy

```
process ENUMERATE {
    tag "${chrom}:${windows.baseName}"
    label 'network'
    maxForks 1                      // not dynamic; per-process concurrency cap

    input:
    tuple val(chrom), path(windows)

    output:
    tuple val(chrom), path("enumerated_${windows.baseName}.tsv.gz"), emit: pairs

    script:
    """
    clingen-index registry enumerate --windows ${windows} --out enumerated.tsv
    gzip -c enumerated.tsv > enumerated_${windows.baseName}.tsv.gz
    """

    stub:
    """
    printf '' | gzip -c > enumerated_${windows.baseName}.tsv.gz
    """
}

process FINALIZE {
    label 'small'
    publishDir "${params.outdir}", mode: 'copy'   // symlink/link fall back to copy on s3:// anyway
    // ...
}
```

#### Config: labels, withName, retry with backoff (strict config: dynamic values must be closures; sleep() resolves inside config closures)

```
process {
    withLabel: network {
        errorStrategy = { sleep(Math.pow(2, task.attempt) * 1000 as long); 'retry' }  // 2 s, 4 s, ...; runs in a synchronized launcher method, keep it short
        maxRetries    = 2
    }
    withLabel: sort {
        cpus   = 4
        memory = 16.GB
    }
    withLabel: genome {
        memory = 8.GB
    }
    withLabel: build {
        memory = 2.GB
    }
    withName: 'ENUMERATE|LOOKUP' {
        maxForks = 1
    }
}
// Priority low->high: process.*  <  directives in process  <  withLabel  <  withName(name)  <  withName(alias)  <  withName('WF:PROC')
```

#### pipeline/nf-test.config (run nf-test from pipeline/)

```
config {
    testsDir   "tests"
    workDir    ".nf-test"                 // add to .gitignore
    configFile "tests/nextflow.config"    // added as -c after ./nextflow.config; not checked for existence, keep it
    profile    "host"                     // CLI --profile X replaces it; --profile +X appends (a blank value here is ignored)
    withTrace  true                       // passes -with-trace; needs procps (ps) where tasks run
    autoSort   true
    requires (
        "nf-test": "0.9.5"                // minimum version: fails if the running nf-test is older
    )
}
```

#### pipeline/tests/nextflow.config (test-only overrides; selectors must match the main config's to win)

```
process {
    withLabel: network {
        errorStrategy = 'terminate'   // no backoff sleeps in tests
        maxRetries    = 0
    }
}
// This file is a later -c than ./nextflow.config, so top-level settings here (e.g. docker.enabled = false)
// also override values set by a profile in nextflow.config (ConfigBuilder merges files in order).
```

#### Process stub test: pipeline/tests/modules/registry.nf.test (snapshot -> registry.nf.test.snap beside it)

```
nextflow_process {

    name    "ENUMERATE (stub)"
    script  "modules/registry.nf"      // no ./ prefix: resolved against nf-test's working directory (run from pipeline/)
    process "ENUMERATE"
    tag     "stub"                     // suite-level tag: --tag stub selects every test in this file
    options "-stub"                    // passed to `nextflow run`; -stub-run also works

    test("emits one gzipped pair file per batch") {
        when {
            params {
                outdir = "$outputDir"
            }
            process {
                """
                input[0] = channel.of( ['21', file("${projectDir}/tests/data/windows_chr21.tsv")] )
                """
            }
        }
        then {
            assert process.success
            assert process.out.pairs.size() == 1
            assert snapshot(process.out).match()
        }
    }
}
```

#### Pipeline (entry workflow) stub + real tests: pipeline/tests/main.nf.test (genome and clinvar_vcf must be declared in the schema, as they now are)

```
nextflow_pipeline {

    name    "pipeline"
    script  "main.nf"
    tag     "pipeline"
    options "-stub"                         // suite default; a test-level options replaces it

    test("stub: chr21 publishes index and reports") {
        tag "stub"
        when {
            params {
                chrom       = "21"          // via -params-file; nf-schema still casts it to Integer for validation
                regions     = "${projectDir}/tests/data/chr21_exons.bed"
                clinvar_vcf = "${projectDir}/tests/data/clinvar_chr21_subset.vcf.gz"
                outdir      = "$outputDir"
            }
        }
        then {
            assert workflow.success
            assert path("$outputDir/index/caid.fst").exists()
            assert snapshot(path("$outputDir/reports").list()).match("reports")
        }
    }

    test("real: chr21 on tiny genome") {
        tag     "real"
        options ""                         // drop -stub for this test
        config  "./real.config"            // extra -c, relative to this test file, applied last
        when {
            params {
                chrom  = "21"
                genome = "${projectDir}/tests/data/tiny_genome.bin"
                outdir = "$outputDir"
            }
        }
        then {
            assert workflow.success
            assert workflow.trace.failed().size() == 0
            assert snapshot(path("$outputDir/harvest/21.tsv.gz")).match("harvest")   // .gz md5 taken on gunzipped content
        }
    }
}
```

#### Running nf-test

```
cd pipeline
nf-test init                                   # once: nf-test.config + tests/nextflow.config
nf-test test                                   # all tests under testsDir, profile from nf-test.config
nf-test test --tag stub                        # case-insensitive, OR over a comma list; also matches suite/test names and suite-level tags
nf-test test tests/main.nf.test --profile local    # replaces config profile -> -profile local
nf-test test --profile +slurm                  # appends -> -profile host,slurm
nf-test test -c nf-test.slurm.config           # alternate nf-test config (own configFile/profile/options)
nf-test test --update-snapshot                 # re-record failing snapshots
nf-test test --ci                              # fail instead of writing new snapshots
nf-test test --verbose --debug                 # --verbose: Nextflow output; --debug: profiles, configs and the exact command

# install (pick one)
curl -fsSL https://get.nf-test.com | bash -s 0.9.5      # writes ./nf-test and ~/.nf-test/nf-test.jar
# conda: bioconda::nf-test=0.9.5  (needs nextflow via NEXTFLOW_HOME, PATH or /usr/local/bin)
```


## SLURM executor protocol and shims

### Findings

- **Version read** (verified): Confirmed. v26.04.7 is the newest stable (non-prerelease) Nextflow release published on or before 2026-10-10: published 2026-10-08T16:28:35Z, and VERSION at the tag reads 26.04.7. The next stable, v26.10.0 (2026-10-12T10:52:45Z), is too new. The later v26.04.8 (2026-10-20) is a prerelease. At v26.10.0, SlurmExecutor.groovy and GridTaskHandler.groovy match v26.04.7 byte for byte (cmp). AbstractGridExecutor.groovy only adds createResourceTracker() for executor.cpus/memory. ADDED: command-run.txt does change in v26.10.0. `.exitcode` is now written after {{sync_cmd}}, and stdio redirections were added to the unstage and launch subshells. The slurm call protocol (sbatch/squeue/scancel, .exitcode as the completion marker) is the same.  
  Source: https://github.com/nextflow-io/nextflow/releases/tag/v26.04.7 ; https://api.github.com/repos/nextflow-io/nextflow/releases?per_page=100 ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/VERSION ; diff of raw files at tags v26.04.7 and v26.10.0 (SlurmExecutor, GridTaskHandler, AbstractGridExecutor, command-run.txt)
- **sbatch command line** (verified): Confirmed. getSubmitCommandLine returns List.of('sbatch', scriptFile.getName()), i.e. `sbatch .command.run` (bare file name, no other arguments). Only when Fusion is on (pipeLauncherScript() returns isFusionEnabled()) does it return List.of('sbatch'), with the script piped on stdin.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/SlurmExecutor.groovy#L124-L129 and #L226-L229
- **sbatch working dir and process handling** (verified): Confirmed. createProcessBuilder sets redirectErrorStream(true), and directory(task.workDir) only when Fusion is off. processStart reads process.text (to EOF) before process.waitFor(), so a job that inherits sbatch's stdout/stderr blocks the submit. A non-zero exit throws ProcessNonZeroExitStatusException with the output as its reason. Failsafe retries only if executor.retry.reason (a regex, applied with find()) matches that output. Defaults from ExecutorRetryConfig: reason 'Socket timed out', maxAttempts 3, delay 500ms, maxDelay 30s, jitter 0.25. Otherwise submit() throws ProcessFailedException "Error submitting process '<name>' for execution". The same happens when parseJobId throws.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/GridTaskHandler.groovy#L109-L125 , #L127-L177 , #L179-L209 , #L254-L283 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/ExecutorRetryConfig.groovy
- **#SBATCH header and chdir** (verified): Confirmed, with one precision. Nextflow never emits --chdir/-D. The order is: `--array 0-N` (arrays only); `-J <name>`; `-o <workdir>/.command.log` (/dev/null for arrays; if Escape.path changes the path, it is backslash-escaped and wrapped in double quotes); `--no-requeue`; `--signal B:USR2@30` (omitted if clusterOptions contains '--signal ' or '--signal='); `-c N` (cpus>1); `-t HH:mm:ss`; `--mem <MB>M` or `--mem-per-cpu`; `-p <queue>`; clusterOptions; `-A <account>`. CORRECTED: the default job name is 'nf-' + task name with exactly these characters replaced by '_': space / : @ * ? =. The "\\n", "\\t", "\\r" entries in INVALID_NAME_CHARS are two-character strings and never match. Quotes, parentheses and other characters pass through. The name is truncated to 256 characters, and an executor.jobName closure overrides it. getHeaderScript then appends `NXF_CHDIR=<Escape.path(workDir)>`, and nxf_main runs `[[ "${NXF_CHDIR:-}" ]] && cd "$NXF_CHDIR"`. .command.run is written with Files.newBufferedWriter and no chmod. The template's first non-'##' line is `#!/bin/bash`.  
  Source: SlurmExecutor.groovy#L57-L113 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/AbstractGridExecutor.groovy#L44 , #L84-L117 , #L163-L183 , #L416-L419 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nf-commons/src/main/nextflow/util/Escape.groovy#L32 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/resources/nextflow/executor/command-run.txt#L15 , #L155-L160 ; BashWrapperBuilder.groovy#L464-L487
- **Job id parsing** (verified): Confirmed. SUBMIT_REGEX = ~/Submitted batch job (\d+)/ is applied with find() to each line, so trailing text such as ' on cluster mpp2' is fine (unit test case). The fallback is text.trim().isLong(), so bare-id `--parsable` output also works. Anything else throws IllegalStateException("Invalid SLURM submit response..."). The id is stored as the trace field native_id.  
  Source: SlurmExecutor.groovy#L40 , #L137-L153 ; GridTaskHandler.groovy#L524-L529 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/test/groovy/nextflow/executor/SlurmExecutorTest.groovy#L44-L62
- **squeue command** (verified): Confirmed. The default call is ['squeue','--noheader','-o','%i %t','-t','all'] + ['-p',queue] (only if the queue is set) + ['-u', System.getProperty('user.name')]. With executor.onlyJobState=true it is the same base plus '--only-job-state', with no -p and no -u. PRECISION: onlyJobState first appeared in 25.12.0-edge (#6659). The docs mark it versionadded 26.04.0, it is absent from v25.10.8, and it needs SLURM 24.05+. executor.queueGlobalStatus=true sets the queue to null, so there is no -p. The call runs with stderr merged and waitForOrKill(60_000). A non-zero exit gives a one-time warn1 and returns null, and so does an exception. Results are kept by Throttle.cache under the key "<executor>_<queue>" for queueStatInterval. A null result is cached too.  
  Source: SlurmExecutor.groovy#L158-L179 ; AbstractGridExecutor.groovy#L285-L337 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nf-commons/src/main/nextflow/util/Throttle.groovy#L207-L228 ; https://raw.githubusercontent.com/nextflow-io/nextflow/v26.04.7/changelog.txt (line 285, 25.12.0-edge) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/config.md (executor.onlyJobState)
- **squeue parsing and state codes** (verified): Confirmed. Each line is split with Java String.split(/\s+/) and must give exactly 2 columns; other lines are skipped with a debug log. A leading blank yields an empty first column (3 columns), so the line is dropped; trailing blanks are harmless. STATUS_MAP: PD and CF map to PENDING; R and CG to RUNNING; CD to DONE; CA, F, TO, NF, PR and BF to ERROR; S and ST to HOLD. Unmapped codes (e.g. OOM, DL) are stored with a null value. checkActiveStatus is true only for RUNNING or HOLD, or when the status map is null (squeue failed). A missing id gives false. checkStartedStatus is true when the map is non-empty, contains the id, and the value is not PENDING (so a null/unmapped state counts as started).  
  Source: SlurmExecutor.groovy#L185-L217 ; AbstractGridExecutor.groovy#L371-L412 ; SlurmExecutorTest.groovy testQstatCommand (L275)
- **How Nextflow decides a job finished** (verified): Confirmed. Start is detected by any of three checks. (1) The `.command.begin` file has a last-modified time; the wrapper runs `touch <Escape.path(workdir)>/.command.begin`. (2) checkStartedStatus is true. (3) `.exitcode` exists, checked only when more than exitReadTimeout has passed since the last such check. On start, the status becomes RUNNING and startedMillis is set to local now. checkIfCompleted reads `.exitcode` only when the task is RUNNING. Its trimmed value is the exit status; a non-integer gives Integer.MAX_VALUE; an empty file is retried for exitReadTimeout, then gives -1. If `.exitcode` is missing, Nextflow returns null (keeps waiting) while elapsed < 2.5 x queueStatInterval or the job is active. Otherwise it starts a timer, and after exitReadTimeout (default 270 s) returns Integer.MAX_VALUE (2147483647). A squeue CD never completes a task by itself. on_exit writes the file with `printf -- $exit_status > <workdir>/.exitcode`. passSanityCheck (throttled to once per exitReadTimeout) fails a SUBMITTED or RUNNING task whose work dir is gone, with 'Task work directory is missing (!)'.  
  Source: GridTaskHandler.groovy#L313-L417 , #L420-L456 , #L458-L497 ; command-run.txt#L106-L124 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/SimpleFileCopyStrategy.groovy#L361-L387 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskRun.groovy#L551-L561
- **Job id disappearing from squeue** (verified): Confirmed by source reading, with one addition; not executed. An id absent from the map makes checkActiveStatus return false (trace log 'map does not contain jobId'). If `.exitcode` exists, its value is used. If not, after 2.5 x queueStatInterval plus exitReadTimeout the task gets exit status 2147483647. The queue dump and work-dir listing are logged at DEBUG level only, and the listing only when FileHelper.workDirIsSharedFS. If the job vanishes before it ever started (no .command.begin, never non-PD), GridTaskHandler has no timeout. The task stays SUBMITTED, `.exitcode` is checked once per exitReadTimeout, and the work-dir sanity check still runs. ADDED: TaskPollingMonitor.checkTaskStatus also calls handler.isSubmitTimeout(). If the `maxSubmitAwait` process directive is set, a task still SUBMITTED after that long is killed (scancel) and failed with ProcessSubmitTimeoutException. It has no default, so without it the task waits indefinitely.  
  Source: AbstractGridExecutor.groovy#L394-L412 ; GridTaskHandler.groovy#L313-L377 , #L435-L456 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskPollingMonitor.groovy#L698-L732 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskHandler.groovy#L396-L404
- **scancel** (verified): Confirmed. getKillCommand() is ['scancel']. A single kill runs `scancel <id>` with stderr merged and waitForOrKill(10_000); a non-zero exit is logged at debug only. cleanup() is registered with session.onShutdown. It hands running grid handlers a BatchCleanup, which runs `scancel id1 id2 ...` in chunks of executor.killBatchSize (default 100). Cleanup is skipped when the session has disableJobsCancellation, set by `-disable-jobs-cancellation` or NXF_DISABLE_JOBS_CANCELLATION. A handler whose status check throws is killed (#5516 comment), and so is a maxSubmitAwait timeout. The wrapper traps TERM, INT and USR2 and ignores USR1. PRECISION: on_term runs `[[ "$pid" ]] && nxf_kill $pid` only for tasks without a container. With a container, kill_cmd comes from containerBuilder.getKillCommand(). Either way the TERM-interrupted `wait` sets nxf_main_ret (143), and on_exit then writes .exitcode.  
  Source: SlurmExecutor.groovy#L156 ; AbstractGridExecutor.groovy#L233-L268 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/BatchCleanup.groovy (kill/kill0) ; TaskPollingMonitor.groovy#L368 , #L618-L638 , #L698-L732 , #L760-L795 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/cli/CmdRun.groovy#L278-L287 ; command-run.txt#L121-L124 , #L155-L158 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/BashWrapperBuilder.groovy#L65 , #L336-L345
- **Settings that make a shim test fast** (verified): Defaults confirmed. The grid pollInterval default is 5 s (createTaskMonitor passes Duration.of('5 sec'); the docs table says 'Grid Executors 100 / 5s'). queueStatInterval defaults to 1min: it is the squeue cache period and the base of the 2.5x grace. exitReadTimeout defaults to 270sec: it is the dead-job wait, the .exitcode check period in isStarted, and the sanity-check period. ExecutorFactory names the executor 'slurm', and getExecConfigProp checks executor.$slurm.<opt> first, then executor.<opt>. CORRECTED: in v26.04.7 the documented `executor { $slurm { ... } }` form works at runtime, but ConfigValidator logs 'WARN: Unrecognized config option executor.$slurm.<opt>' for each key. isSelector knows only withLabel:/withName:, and SpecNode has no $-scope (issue #7551, reported on 26.04.6). The fix, PR #7555 (merged 2026-09-01), is in v26.10.0's ConfigValidator but not in v26.04.7, whose changelog does not list it. A test-only config should therefore use plain executor.<opt> keys.  
  Source: AbstractGridExecutor.groovy#L52-L64 ; TaskPollingMonitor.groovy#L165-L175 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/ExecutorConfig.groovy#L171-L232 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/executor/ExecutorFactory.groovy#L54 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/config/ConfigValidator.groovy#L100-L153 ; https://github.com/nextflow-io/nextflow/issues/7551 ; https://github.com/nextflow-io/nextflow/pull/7555 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/config.md (executor section, 'Executor-specific defaults' and 'Executor-specific configuration')
- **NXF_EXECUTOR_* environment fallback** (likely): From source, not executed. getExecConfigProp reads NXF_EXECUTOR_<NAME uppercased, dots to underscores> only when both executor.$<exec>.<name> and the ExecutorConfig property are null. ExecutorFactory always builds ExecutorConfig with the Map constructor. That constructor gives queueStatInterval (1min), exitReadTimeout (270sec), dumpInterval (5min) and killBatchSize (100) non-null defaults, and the boolean options (onlyJobState, queueGlobalStatus, perCpuMemAllocation) are primitives. So env vars cannot change any of these. pollInterval and queueSize have no constructor default, so NXF_EXECUTOR_POLLINTERVAL / NXF_EXECUTOR_QUEUESIZE would apply. NXF_EXECUTOR_* is not listed in docs/reference/env-vars.md (only NXF_EXECUTOR, the default executor).  
  Source: ExecutorConfig.groovy#L171-L232 ; ExecutorFactory.groovy#L208 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/env-vars.md
- **Shim behaviour as tested on this Mac (bash 3.2.57, macOS 27.0.1)** (verified): Re-tested independently: my own renderer of v26.04.7 command-run.txt (no container), shims copied from scratchpad/shims. Confirmed: `sbatch .command.run` from the work dir, with stdin held open by a pipe and output read to EOF, returned in 0.06-0.24 s with 'Submitted batch job 100N'. `squeue --noheader -o '%i %t' -t all -p normal -u <user>` and the `--only-job-state` form showed R, then CD (exit 0) or F (exit 3). `.command.begin`, `.command.log` and `.exitcode` (0/3/143) were written, including a work dir whose path contains a space (quoted, escaped -o). Every job process was in a process group whose pgid is the runner pid. scancel of a normal job showed CA (CG was too short to observe) and left the group empty. Also confirmed: arrays and missing files refused with exit 1; unknown id in scancel exits 1; -D in #SBATCH, CLI -D wins; %x/%j/%% expansion; --parsable; --wrap; stdin; -p/-u filters; FAKE_SLURM_HIDE_FINISHED; 30 parallel submits gave 30 unique ids; `bash -n` passes; shellcheck is not installed. CORRECTED: 'no job process survived' does not hold when the task script ignores TERM (e.g. `trap "" TERM; sleep 40` in .command.sh). The wrapper exits 143 and the runner writes rc, and the original scancel's delayed SIGKILL is skipped because of its `[ -f "$d/rc" ] ||` guard. The task's bash and sleep survived as orphans. A plain batch script that ignores TERM was killed after FAKE_SLURM_KILL_WAIT, as claimed. With the guard removed (corrected scancel snippet), no process survived in either case. ALSO FOUND: an apostrophe in a #SBATCH line (e.g. a task name with a quote, which Nextflow does not sanitise) makes xargs print 'xargs: unterminated quote'. That line's option is lost, but submit still succeeds; Nextflow would see the message in the merged sbatch output, and parseJobId still finds the id. Linux bash is untested.  
  Source: Local tests: /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/verify-shim-test/tools/render.py , runtest.sh , runtest2.sh ; shims under test /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/shims/ (patched scancel copy in /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/verify-shim-test/bin/scancel)
- **Design choices in the shims** (verified): Confirmed by test and docs, with one fix. Under `set -m` the job runs in its own process group (pgid = runner pid, seen in ps). scancel signals -pgid with SIGCONT then SIGTERM, then SIGKILL after a wait, the same sequence SLURM's scancel documents (KillWait default 30 s per slurm.conf docs). `trap ':' TERM` in the runner, rather than an ignore, lets the wrapper trap TERM: the wrapper wrote 143. sbatch's stdio is detached from the job. Ids come from atomic mkdir. F for a non-zero exit matches squeue's documented 'F FAILED Job terminated with non-zero exit code'. For Nextflow it behaves like CD: both are non-active, and the exit status comes from .exitcode. To always show CD, change the `else echo F` inside the rc branch of job_state. FIX: the delayed SIGKILL in scancel must not be skipped when rc exists. SLURM kills every remaining job process after KillWait, so the background subshell should run `kill -s KILL -- "-$pid"` unconditionally (an empty group just fails silently). Remaining gap: on normal completion without scancel, processes a task left in the background are not killed (SLURM would kill them at job end). This is irrelevant for stub runs.  
  Source: shim sources (snippets) ; https://slurm.schedmd.com/scancel.html ; https://slurm.schedmd.com/slurm.conf.html#OPT_KillWait ; https://slurm.schedmd.com/squeue.html (JOB STATE CODES) ; local test runtest2.sh
- **Spec interaction: slurm profile uses Apptainer** (verified): Confirmed, with additions. The spec's slurm profile uses Apptainer and `host` is the no-container add-on. A stub run keeps the container: TaskRun.resolve swaps only the script via resolveStub, and nothing in the stub path touches the container settings. So on this Mac the shim test must turn containers off. ADDED: the strict config parser is on by default since 26.04.0, and under it profiles apply in command-line order. So `-profile slurm,host` lets host's settings win. Under NXF_SYNTAX_PARSER=v1, profiles apply in the order they are defined in the config. So define `host` after `slurm` in nextflow.config as well, or also set apptainer.enabled=false in the test config given with -c. -c files have the highest documented priority, though I did not run that override. --slurm_queue is optional for the shim, which works without -p. Passing `--slurm_queue normal` exercises both the `#SBATCH -p` header and the `squeue -p` filter.  
  Source: docs/superpowers/specs/2026-10-24-index-regeneration-pipeline-design.md (Profiles and run examples; Testing: Profiles) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/modules/nextflow/src/main/groovy/nextflow/processor/TaskRun.groovy#L831-L835 , #L879-L895 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/config.md#L7-L12 , #L320-L324 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/env-vars.md (NXF_SYNTAX_PARSER: strict parser enabled by default in 26.04.0)

### Open questions the researchers raised

- Not yet run under a real Nextflow: it is not installed here and nothing was installed. Both test rounds (the colleague's and mine) emulate Nextflow's calls against wrappers rendered from the v26.04.7 template without trace, container or scratch. The first real `-profile slurm,host -stub-run` run is the confirmation.
- Tested only with macOS /bin/bash 3.2.57 on macOS 27.0.1. The shims avoid bash 4 features and GNU-only flags, but Linux bash and GNU xargs are untested. GNU xargs runs printf once on empty input, which is harmless here.
- The shims do not support job arrays (Nextflow's `array` directive; sbatch exits 1). They do not enforce -t time limits, the `--signal B:USR2@30` warning signal, or cpu/memory limits. Fine for the spec, which uses no arrays.
- Leftover background processes of a task that completes normally (no scancel) are not killed by the shim, whereas SLURM would kill them at job end. This is irrelevant for stub runs. The scancel fix covers only the cancel path.
- A quote character (' or ") in a #SBATCH line, e.g. from a task name or tag (Nextflow's -J sanitising keeps quotes), makes the shim's xargs print 'xargs: unterminated quote' and drop that line's option. The submit still succeeds. Spec task names (e.g. 'ENUMERATE (21)') are unaffected.
- Not run: whether a plain `apptainer.enabled = false` in a -c file overrides the slurm profile's setting from nextflow.config. The docs give -c files the highest priority, but I did not execute it.
- Source reading only: NXF_EXECUTOR_* env vars apply only to options without a constructor default (pollInterval, queueSize). They do not apply to queueStatInterval, exitReadTimeout, dumpInterval, killBatchSize or the boolean options, so the test should set these in the config file.
- Source reading only: a job that vanishes from squeue before it ever starts stays SUBMITTED indefinitely unless the `maxSubmitAwait` directive is set.

### Snippets

#### Exact calls Nextflow v26.04.7 makes (slurm executor)

```
# submit (cwd = task work dir; stdout+stderr merged, read to EOF)
sbatch .command.run
#   parse: /Submitted batch job (\d+)/ per line, else whole output if it is a long
# status (cached per '<executor>_<queue>' for executor.queueStatInterval; no -p if queueGlobalStatus)
squeue --noheader -o '%i %t' -t all [-p <queue>] -u <user.name>
squeue --noheader -o '%i %t' -t all --only-job-state      # executor.onlyJobState = true
#   expects '<id> <code>' lines, exactly 2 whitespace-separated columns
#   PD,CF=PENDING  R,CG=RUNNING  CD=DONE  CA,F,TO,NF,PR,BF=ERROR  S,ST=HOLD
# kill (10 s timeout); on shutdown batched up to executor.killBatchSize ids
scancel <id> [<id> ...]
# completion = <workdir>/.exitcode (integer); start = <workdir>/.command.begin
```

#### sbatch (shim)

```
#!/bin/bash
# Fake `sbatch` for testing Nextflow's slurm executor on a machine without SLURM.
#
# Runs the batch script locally, in the background, in its own process group,
# and prints "Submitted batch job <id>" (or just "<id>" with --parsable).
# Honours #SBATCH and command-line -D|--chdir, -o|--output, -e|--error,
# -J|--job-name, -p|--partition, -c|--cpus-per-task, --wrap, --parsable;
# other options are accepted and ignored. Job arrays (-a|--array) are refused.
# Command-line options override #SBATCH directives, as in real sbatch.
#
# State: $FAKE_SLURM_DIR (default ${TMPDIR:-/tmp}/fake-slurm-<uid>), one
# directory per job holding: pid, rc (written when the script exits),
# cancelled (written by scancel), name, partition, user, workdir.
# Works with macOS /bin/bash 3.2 and Linux bash; no GNU-only tools.

set -u

die() { printf 'sbatch: error: %s\n' "$*" >&2; exit 1; }

state=${FAKE_SLURM_DIR:-${TMPDIR:-/tmp}/fake-slurm-$(id -u)}
state=${state%/}
mkdir -p "$state/jobs" || die "cannot create state directory $state/jobs"

# Options that take a value. Short: all but sbatch's flag letters
# (-h -H -I -k -O -Q -s -u -v -V -W). Long: the common valued options;
# an unknown long option without '=' is treated as a flag.
takes_value() {
    case $1 in
        -[hHIkOQsuvVW]) return 1 ;;
        -?) return 0 ;;
        --account|--array|--begin|--chdir|--clusters|--comment|--constraint|\
        --cpus-per-gpu|--cpus-per-task|--dependency|--distribution|--error|\
        --exclude|--export|--gpus|--gres|--input|--job-name|--licenses|\
        --mail-type|--mail-user|--mem|--mem-per-cpu|--mem-per-gpu|--nodelist|\
        --nodes|--ntasks|--ntasks-per-node|--output|--partition|--qos|\
        --reservation|--signal|--time|--tmp|--wckey|--wrap) return 0 ;;
    esac
    return 1
}

o_chdir='' o_out='' o_err='' o_name='' o_part='' o_cpus='' o_array='' o_wrap='' o_parsable=0

set_opt() { # <option> <value>
    case $1 in
        -D|--chdir)         o_chdir=$2 ;;
        -o|--output)        o_out=$2 ;;
        -e|--error)         o_err=$2 ;;
        -J|--job-name)      o_name=$2 ;;
        -p|--partition)     o_part=$2 ;;
        -c|--cpus-per-task) o_cpus=$2 ;;
        -a|--array)         o_array=$2 ;;
        --wrap)             o_wrap=$2 ;;
        --parsable)         o_parsable=1 ;;
    esac
}

# parse_opts ARGS... : applies the leading options in ARGS and sets
# `consumed` to how many arguments they used (the script path comes next).
parse_opts() {
    consumed=0
    while [ $# -gt 0 ]; do
        case $1 in
            --) consumed=$((consumed + 1)); return ;;
            --*=*) set_opt "${1%%=*}" "${1#*=}" ;;
            --*|-?)
                if takes_value "$1"; then
                    [ $# -ge 2 ] || die "option '$1' requires an argument"
                    set_opt "$1" "$2"; shift; consumed=$((consumed + 1))
                else
                    set_opt "$1" ''
                fi ;;
            -?*) set_opt "${1:0:2}" "${1:2}" ;;   # attached value: -o/path, -c2
            *) return ;;
        esac
        shift; consumed=$((consumed + 1))
    done
}

unescape() { printf '%s' "$1" | sed 's/\\\(.\)/\1/g'; }   # "/a\ b" -> "/a b"

submit_dir=$PWD

# Command line, pass 1: find where the options end and the script begins.
parse_opts "$@"
ncli=$consumed
cli=("$@")
shift "$ncli"

# Allocate a job id atomically (mkdir is atomic, so concurrent calls are safe).
id=$(cat "$state/last_id" 2>/dev/null)
case $id in ''|*[!0-9]*) id=1000 ;; esac
while :; do
    id=$((id + 1))
    mkdir "$state/jobs/$id" 2>/dev/null && break
done
printf '%s\n' "$id" > "$state/last_id"
jobdir=$state/jobs/$id

# The script: --wrap, a file argument, or stdin. (Nextflow passes the file
# name `.command.run`; it pipes the script on stdin only when Fusion is on.)
if [ -n "$o_wrap" ]; then
    printf '#!/bin/sh\n%s\n' "$o_wrap" > "$jobdir/script"
    script=$jobdir/script
elif [ $# -gt 0 ]; then
    script=$1; shift
    case $script in /*) ;; *) script=$submit_dir/$script ;; esac
    if [ ! -r "$script" ]; then rm -rf "$jobdir"; die "Unable to open file $script"; fi
else
    cat > "$jobdir/script"
    script=$jobdir/script
fi

# Pass 2: #SBATCH directives. Like sbatch, stop at the first line that is
# neither blank nor a comment. Then pass 3: the command line again, so it wins.
# Note: xargs rejects an unbalanced ' or " ("unterminated quote"); that line's
# option is then lost but the submit still succeeds.
while IFS= read -r line || [ -n "$line" ]; do
    case $line in
        '#SBATCH'[[:space:]]*)
            toks=()
            # xargs splits words honouring quotes and never executes anything
            while IFS= read -r t; do toks[${#toks[@]}]=$t; done \
                < <(printf '%s\n' "${line#\#SBATCH}" | xargs printf '%s\n')
            [ ${#toks[@]} -gt 0 ] && parse_opts "${toks[@]}" ;;
        '#'*|'') ;;
        *) break ;;
    esac
done < "$script"
[ "$ncli" -gt 0 ] && parse_opts "${cli[@]:0:$ncli}"

if [ -n "$o_array" ]; then
    rm -rf "$jobdir"; die "job arrays (--array $o_array) are not supported by this shim"
fi

name=${o_name:-$(basename "$script")}
user=$(id -un)
part=${o_part:-${FAKE_SLURM_PARTITION:-normal}}
workdir=$(unescape "${o_chdir:-$submit_dir}")
case $workdir in /*) ;; *) workdir=$submit_dir/$workdir ;; esac

# Output files: default slurm-%j.out; no -e means stderr goes with stdout.
# Relative paths resolve against the job's working directory (the runner
# cd's there before opening them). %j, %x, %u are expanded; %% is a '%'.
expand() {
    local p pct=$'\001'
    p=$(unescape "$1")
    p=${p//\%\%/$pct}
    p=${p//\%j/$id}; p=${p//\%x/$name}; p=${p//\%u/$user}
    p=${p//$pct/%}
    printf '%s' "$p"
}
out=$(expand "${o_out:-slurm-%j.out}")
if [ -n "$o_err" ]; then err=$(expand "$o_err"); else err=$out; fi

printf '%s\n' "$name" > "$jobdir/name"
printf '%s\n' "$part" > "$jobdir/partition"
printf '%s\n' "$user" > "$jobdir/user"
printf '%s\n' "$workdir" > "$jobdir/workdir"

# Interpreter from the shebang, as slurmd would use (Nextflow: #!/bin/bash).
interp=/bin/sh
IFS= read -r first < "$script" || true
case $first in '#!'*) interp=${first#\#!} ;; esac

export SLURM_JOB_ID=$id SLURM_JOBID=$id SLURM_JOB_NAME=$name \
       SLURM_JOB_PARTITION=$part SLURM_SUBMIT_DIR=$submit_dir SLURM_JOB_USER=$user
[ -n "$o_cpus" ] && export SLURM_CPUS_PER_TASK=$o_cpus

# Launch. `set -m` puts the job in its own process group (pgid = runner pid)
# so scancel can signal the whole tree. The runner traps TERM (a trap, not
# an ignore, so the job's own TERM trap still works) and survives to record
# the exit code. All of sbatch's stdio is detached from the job: Nextflow
# reads sbatch's output until EOF, so an inherited pipe would block submit.
set -m
(
    trap '' HUP
    trap ':' TERM
    rc=1
    if cd "$workdir"; then
        set -f
        # shellcheck disable=SC2086  # interp may be "/usr/bin/env bash"
        if [ "$err" = "$out" ]; then
            $interp "$script" "$@" > "$out" 2>&1 < /dev/null
        else
            $interp "$script" "$@" > "$out" 2> "$err" < /dev/null
        fi
        rc=$?
    fi
    printf '%s\n' "$rc" > "$jobdir/rc.tmp" && mv -f "$jobdir/rc.tmp" "$jobdir/rc"
) < /dev/null > /dev/null 2>&1 &
pid=$!
set +m
printf '%s\n' "$pid" > "$jobdir/pid"

if [ "$o_parsable" = 1 ]; then
    printf '%s\n' "$id"
else
    printf 'Submitted batch job %s\n' "$id"
fi
```

#### squeue (shim)

```
#!/bin/bash
# Fake `squeue` for the fake `sbatch`/`scancel` shims (see sbatch).
#
# Nextflow (slurm executor) runs:
#   squeue --noheader -o '%i %t' -t all [-p <queue>] -u <user>
#   squeue --noheader -o '%i %t' -t all --only-job-state   (onlyJobState=true)
# and expects lines "<jobid> <state>" with no leading blanks.
#
# State codes: PD before the runner pid is recorded; R while the job runs;
# CG while a cancelled job is still shutting down; then CD (exit 0),
# F (non-zero exit, as real SLURM reports a failed batch script) or CA
# (cancelled). Supported options: -h/--noheader, -o/--format (%i %A %t %T
# %j %P %u %%, widths like %.18i ignored), -t/--states (default: pending and
# running only; 'all' or a comma list), -p/--partition, -u/--user, --me,
# -j/--jobs; others (e.g. --only-job-state) are accepted and ignored.
# FAKE_SLURM_HIDE_FINISHED=1 drops finished jobs from every listing, to
# simulate SLURM forgetting them (MinJobAge).

set -u

state=${FAKE_SLURM_DIR:-${TMPDIR:-/tmp}/fake-slurm-$(id -u)}
state=${state%/}

header=1 fmt='%.18i %.9P %.8j %.8u %.2t' states='' users='' parts='' ids=''

need() { [ $# -ge 2 ] || { printf 'squeue: error: option %s requires an argument\n' "$1" >&2; exit 1; }; }

while [ $# -gt 0 ]; do
    case $1 in
        -h|--noheader)   header=0 ;;
        -o|--format)     need "$@"; fmt=$2; shift ;;
        --format=*)      fmt=${1#*=} ;;
        -o?*)            fmt=${1#-o} ;;
        -t|--states)     need "$@"; states=$2; shift ;;
        --states=*)      states=${1#*=} ;;
        -p|--partition)  need "$@"; parts=$2; shift ;;
        --partition=*)   parts=${1#*=} ;;
        -u|--user)       need "$@"; users=$2; shift ;;
        --user=*)        users=${1#*=} ;;
        --me)            users=$(id -un) ;;
        -j|--jobs)       need "$@"; ids=$2; shift ;;
        --jobs=*)        ids=${1#*=} ;;
        -M|--clusters|-A|--account|-q|--qos|-w|--nodelist|-n|--name|-L|--licenses|-R|--reservation|-S|--sort|-i|--iterate|-O|--Format)
                         need "$@"; shift ;;
        *)               ;;   # --only-job-state, -a, -l, -r, --array, ...
    esac
    shift
done

# Drop width/justification modifiers: %.18i -> %i, %-10j -> %j
fmt=$(printf '%s' "$fmt" | sed 's/%[-.0-9]*\([A-Za-z%]\)/%\1/g')
states=$(printf '%s' "$states" | tr '[:lower:]' '[:upper:]')

render() { # <id> <st> <name> <partition> <user> <STATE>
    local f=$fmt i=0 c out=''
    while [ "$i" -lt "${#f}" ]; do
        c=${f:$i:1}
        if [ "$c" = '%' ] && [ $((i + 1)) -lt "${#f}" ]; then
            i=$((i + 1)); c=${f:$i:1}
            case $c in
                i|A) out=$out$1 ;;
                t)   out=$out$2 ;;
                j)   out=$out$3 ;;
                P)   out=$out$4 ;;
                u)   out=$out$5 ;;
                T)   out=$out$6 ;;
                %)   out=$out% ;;
                *)   out=${out}N/A ;;
            esac
        else
            out=$out$c
        fi
        i=$((i + 1))
    done
    printf '%s\n' "$out"
}

in_list() { # <value> <comma-list>
    case ",$2," in *",$1,"*) return 0 ;; esac
    return 1
}

long_state() {
    case $1 in
        PD) echo PENDING ;; R) echo RUNNING ;; CG) echo COMPLETING ;;
        CD) echo COMPLETED ;; F) echo FAILED ;; CA) echo CANCELLED ;;
        *) echo UNKNOWN ;;
    esac
}

job_state() { # <jobdir>
    local d=$1 pid='' rc=''
    [ -f "$d/pid" ] && pid=$(cat "$d/pid")
    [ -f "$d/rc" ] && rc=$(cat "$d/rc")
    if [ -n "$rc" ]; then
        if [ -f "$d/cancelled" ]; then echo CA
        elif [ "$rc" = 0 ]; then echo CD
        else echo F
        fi
    elif [ -z "$pid" ]; then
        echo PD
    elif kill -0 "$pid" 2> /dev/null; then
        if [ -f "$d/cancelled" ]; then echo CG; else echo R; fi
    elif [ -f "$d/cancelled" ]; then
        echo CA          # killed (e.g. SIGKILL) before it could record rc
    else
        echo F           # runner vanished without recording rc
    fi
}

[ "$header" = 1 ] && render JOBID ST NAME PARTITION USER STATE

[ -d "$state/jobs" ] || exit 0
for d in "$state"/jobs/*; do
    [ -d "$d" ] || continue
    id=${d##*/}
    case $id in ''|*[!0-9]*) continue ;; esac
    [ -n "$ids" ] && ! in_list "$id" "$ids" && continue

    st=$(job_state "$d")
    lst=$(long_state "$st")
    case $st in
        CD|F|CA) [ "${FAKE_SLURM_HIDE_FINISHED:-0}" = 1 ] && continue ;;
    esac
    if [ -z "$states" ]; then
        case $st in PD|R|CG) ;; *) continue ;; esac
    elif [ "$states" != ALL ]; then
        in_list "$st" "$states" || in_list "$lst" "$states" || continue
    fi

    name=$(cat "$d/name" 2> /dev/null)
    part=$(cat "$d/partition" 2> /dev/null)
    user=$(cat "$d/user" 2> /dev/null)
    [ -n "$parts" ] && ! in_list "$part" "$parts" && continue
    [ -n "$users" ] && ! in_list "$user" "$users" && continue

    render "$id" "$st" "$name" "$part" "$user" "$lst"
done
exit 0
```

#### scancel (shim) -- CORRECTED: delayed SIGKILL no longer skipped when rc exists

```
#!/bin/bash
# Fake `scancel` for the fake `sbatch`/`squeue` shims (see sbatch).
#
# Nextflow runs `scancel <id> [<id>...]` (up to executor.killBatchSize ids,
# default 100) and waits at most 10 s for it. Like SLURM, this sends SIGCONT
# and SIGTERM to every process of the job (its process group), then SIGKILL
# to whatever is left after $FAKE_SLURM_KILL_WAIT seconds (default 5; SLURM's
# KillWait default is 30), in the background, so scancel itself returns at
# once. The job is marked cancelled: squeue shows CG while it shuts down,
# then CA.
# -s/--signal SIG only delivers SIG (no cancel, no SIGKILL), as in SLURM.
# Unknown ids print an error and make the exit status 1; finished jobs are
# skipped. Filters (-u, -p, -n, ...) are not supported and ignored.

set -u

state=${FAKE_SLURM_DIR:-${TMPDIR:-/tmp}/fake-slurm-$(id -u)}
state=${state%/}
wait_s=${FAKE_SLURM_KILL_WAIT:-5}

sig='' status=0 ids=''

while [ $# -gt 0 ]; do
    case $1 in
        -s|--signal) [ $# -ge 2 ] || { echo 'scancel: error: option requires an argument -- s' >&2; exit 1; }
                     sig=$2; shift ;;
        --signal=*)  sig=${1#*=} ;;
        -s?*)        sig=${1#-s} ;;
        -u|--user|-p|--partition|-n|--name|-t|--state|-A|--account|-q|--qos|-w|--nodelist|-M|--clusters|-R|--reservation)
                     printf 'scancel: warning: %s is ignored by this shim\n' "$1" >&2; shift ;;
        -*)          ;;   # -b/--batch, -f/--full, -Q, -v, -i, ...
        *)           ids="$ids $1" ;;
    esac
    shift
done
sig=${sig#SIG}

signal_job() { # <pgid> <signal> : the whole process group, else the leader
    kill -s "$2" -- "-$1" 2> /dev/null || kill -s "$2" "$1" 2> /dev/null
}

for id in $ids; do
    jid=${id%%.*}                       # 123.batch -> 123
    d=$state/jobs/$jid
    case $jid in ''|*[!0-9]*) d='' ;; esac
    if [ -z "$d" ] || [ ! -d "$d" ]; then
        printf 'scancel: error: Kill job error on job id %s: Invalid job id specified\n' "$id" >&2
        status=1
        continue
    fi
    [ -f "$d/rc" ] && continue          # already finished
    pid=$(cat "$d/pid" 2> /dev/null)
    [ -n "$pid" ] || continue
    kill -0 "$pid" 2> /dev/null || continue

    if [ -n "$sig" ]; then
        signal_job "$pid" "$sig"
        continue
    fi

    : > "$d/cancelled"
    signal_job "$pid" CONT
    signal_job "$pid" TERM
    (
        sleep "$wait_s"
        # Unconditional: the Nextflow wrapper can exit (and rc be written) while
        # task processes that ignore TERM are still alive in the group.
        kill -s KILL -- "-$pid" 2> /dev/null   # leftovers too, as SLURM does after KillWait
    ) < /dev/null > /dev/null 2>&1 &
done
exit $status
```

#### Test-only config and run for the slurm profile through the shims -- CORRECTED: plain executor keys (no $slurm warning in 26.04.7)

```
// e.g. pipeline/conf/slurm-shim.config (test only; used only together with -profile slurm,...)
// Plain executor.* keys on purpose: in 26.04.7 `executor { $slurm { ... } }` works but
// ConfigValidator warns "Unrecognized config option 'executor.$slurm.<opt>'"
// (nextflow-io/nextflow#7551; fix #7555 is in 26.10.0, not 26.04.x).
executor {
    pollInterval      = '250ms'  // grid default 5s; completion is seen on the next poll after .exitcode
    queueStatInterval = '1s'     // default 1min; squeue cache + 2.5x grace
    exitReadTimeout   = '10s'    // default 270sec; dead-without-.exitcode -> fail after this
}
// Stub runs keep the container, and this Mac has no Apptainer. The host profile should
// turn it off; this line also covers NXF_SYNTAX_PARSER=v1, where profile order is the
// definition order in the config rather than the -profile order.
apptainer.enabled = false

# shell (host listed last: the strict parser, default since 26.04.0, applies profiles in CLI order)
chmod +x pipeline/tests/slurm-shim/sbatch pipeline/tests/slurm-shim/squeue pipeline/tests/slurm-shim/scancel
export FAKE_SLURM_DIR="$(mktemp -d)"
export PATH="$PWD/pipeline/tests/slurm-shim:$PATH"
nextflow run pipeline -profile slurm,host -stub-run -c pipeline/conf/slurm-shim.config \
  --chrom 21 --slurm_queue normal --outdir "$FAKE_SLURM_DIR/out"
squeue --noheader -o '%i %t' -t all     # expect every job CD
ls "$FAKE_SLURM_DIR/jobs" | wc -l       # expect one job per task
```


## AWS Batch

### Findings

- **Nextflow version used as the docs baseline** (verified): CONFIRMED. The newest stable Nextflow released on or before 2026-10-10 is v26.04.7, published 2026-10-08T16:28:35Z. These are all too new: v26.10.0 (2026-10-12T10:52:45Z), v26.10.1 and v26.10.2 (2026-10-15), v26.10.3 (2026-10-20), v26.04.8 (2026-10-20, which GitHub marks as a prerelease), v26.04.9 (2026-10-22) and also v25.10.9 (2026-10-20). All Nextflow facts below were re-read at tag v26.04.7. https://docs.seqera.io/nextflow/executor has no AWS Batch section; it mentions AWS Batch only inside a local-plus-Batch example. On master, docs/reference/executor/aws-batch.mdx exists, and I spot-checked it on the container requirement, the S3 work directory needing a subdirectory, the IAM list, the Fusion tip, the grandparent mount warning, the entrypoint troubleshooting entry and maxSpotAttempts (0, or 5 with Fusion snapshots). It agrees on all of them.  
  Source: https://api.github.com/repos/nextflow-io/nextflow/releases?per_page=40 ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/aws.md ; https://github.com/nextflow-io/nextflow/blob/master/docs/reference/executor/aws-batch.mdx ; https://docs.seqera.io/nextflow/executor
- **(1) AWS CLI v2 in the image at /usr/local/bin/aws works without aws.batch.cliPath** (verified): CONFIRMED, with one detail added. AwsOptions.getAwsCli() (lines 139-144) returns cliPath, or the bare string 'aws' when cliPath is unset. It also appends ' --region <region>' whenever a region is resolved, so with aws.region set the command is `aws --region us-east-1 s3 cp ...`. AwsBatchExecutor.getLaunchCommand (lines 353-360) returns ['bash','-o','pipefail','-c', s3Cmd]. s3Cmd (lines 362-400) is `trap ... TERM; trap "{ ret=$?; <aws> s3 cp --only-show-errors .command.log <workdir>/.command.log||true; exit $ret; }" EXIT; <aws> s3 cp --only-show-errors <workdir>/.command.run - | bash > >(tee .command.log) 2>&1 & pid=$!; wait $pid`. S3BashLib starts with cli = 'aws' and is given opts.awsCli. So `aws` is looked up on the container's PATH. For debian:bookworm-slim I fetched the linux/amd64 image config from registry-1.docker.io: created 2026-10-20T21:32:55Z, Env PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin, Entrypoint [], Cmd ['bash']. The aws/install script inside the 2.37.12 zip defaults to --install-dir /usr/local/aws-cli and --bin-dir /usr/local/bin (lines 31-36 and 69-70).  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/plugins/nf-amazon/src/main/nextflow/cloud/aws/batch/AwsOptions.groovy ; .../batch/AwsBatchExecutor.groovy ; .../aws/util/S3BashLib.groovy (lines 36, 43-47, 186-193) ; https://registry-1.docker.io/v2/library/debian/manifests/bookworm-slim (amd64 config blob) ; aws/install inside https://awscli.amazonaws.com/awscli-exe-linux-x86_64-2.37.12.zip
- **(1) Do NOT set cliPath to the in-image CLI** (verified): CONFIRMED. AwsBatchTaskHandler lines 598-601: when opts.cliPath is set and Fargate is off, it adds the mount 'aws-cli' -> "<Paths.get(cliPath).parent.parent>:<same>:ro", a host path bind-mounted into the container read-only. AwsBatchConfig.parseCliPath only requires an absolute path ending in '/bin/aws'. So cliPath='/usr/local/bin/aws' passes validation and would mount the host's /usr over the container's /usr. Docs (aws.md line 348): 'The grandparent directory of the `aws` tool will be mounted into the container at the same path as the host ... which will shadow existing files in the container.' The config reference describes cliPath as 'The path where the AWS command line tool is installed in the host AMI.'  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/plugins/nf-amazon/src/main/nextflow/cloud/aws/batch/AwsBatchTaskHandler.groovy (lines 597-612) ; .../aws/config/AwsBatchConfig.groovy (parseCliPath, lines 171-180) ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/aws.md (line 348) ; .../docs/reference/config.md (aws.batch.cliPath)
- **(1) Container requirements: entrypoint, bash, ps and other tools** (verified): CONFIRMED. aws.md lines 492-497 (Troubleshooting) say a 'terminated for an unknown reason' error is commonly caused by 'a non standard entrypoint which does not allow the execution of the Bash launcher script', and 'may also happen if the AWS CLI doesn't run correctly'. bash is required: the job command is `bash -o pipefail -c`, and the command-run.txt template's shebang is #!/bin/bash (template line 15, after the ## licence header). ps is required: nxf_kill() runs `ps -e -o pid= -o ppid=` (line 57). reports.md line 217 lists `awk`, `date`, `grep`, `ps`, `sed`, `tail`, `tee` as needed for trace metrics. executor.md says every process must set the `container` directive. The spec's runtime packages (bash, coreutils, gawk, procps, no ENTRYPOINT) cover these; grep, sed, tail and tee come from Debian's essential set and coreutils.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/aws.md ; .../modules/nextflow/src/main/resources/nextflow/executor/command-run.txt ; .../docs/reports.md ; .../docs/executor.md (AWS Batch section)
- **(1) AWS CLI v2 install prerequisites and the pinned version** (verified): CONFIRMED, independently re-downloaded. The AWS install guide lists the Linux prerequisites as an unzip tool plus glibc, groff and less. It also says the command-line installer 'is a good option for version control because you can specify the version to install'. getting-started-version.html documents the pattern: append '-<version>' to the file name, e.g. awscli-exe-linux-x86_64-2.0.30.zip and awscli-exe-linux-aarch64-2.0.30.zip. The newest aws-cli tag on or before 2026-10-10 is 2.37.12 (commit 'Bump version to 2.37.12', 2026-10-09T18:09:23Z). 2.37.13 (2026-10-12) and 2.38.x are too new. HEAD and GET results: x86_64 zip is 73,886,092 B, Last-Modified Fri, 09 Oct 2026 19:05:44 GMT, sha256 e6d3c672fb36ed4b12c354c39a993eee2b34da1437cafb10bac4480840cfa83f. aarch64 zip is 71,337,894 B, Last-Modified Fri, 09 Oct 2026 19:07:29 GMT, sha256 829c76096e05f743267d75098bc7cc7cfae799fc7e568554ebaa6d78e6aba753. Both .zip.sig files return HTTP 200 (566 B). The GPG signature was NOT verified, because no gpg, gpg2, sq or sqv is installed here. AWS's key ID is A6310ACC4672475C (fingerprint FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C). Note: the guide now calls a new https://awscli.amazonaws.com/v2/install.sh script the 'recommended' method, but the pinned zip is the documented way to fix a version.  
  Source: https://api.github.com/repos/aws/aws-cli/tags + /commits/<sha> ; HEAD/GET https://awscli.amazonaws.com/awscli-exe-linux-{x86_64,aarch64}-2.37.12.zip(.sig) ; https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html ; https://docs.aws.amazon.com/cli/latest/userguide/getting-started-version.html
- **(1) Fusion alternative** (verified): CONFIRMED. aws.md line 169 tip: 'The recommended approach for running Nextflow pipelines with AWS Batch is to use Wave and Fusion for S3 access, rather than relying on the AWS CLI.' Line 305: 'The need for the AWS CLI is considered a legacy requirement'. Master aws-batch.mdx line 307: 'With Fusion, the container images and the AMI do not need the AWS CLI.'  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/aws.md ; https://github.com/nextflow-io/nextflow/blob/master/docs/reference/executor/aws-batch.mdx
- **(2) awsbatch config keys** (verified): CONFIRMED, with two corrections. Confirmed: process.executor = 'awsbatch'; process.queue (SubmitJob jobQueue accepts 'either the name or the ARN'); process.container is required for every process. aws.batch.maxParallelTransfers: 'Max parallel upload/download transfer operations *per job*' (default 4). aws.batch.maxSpotAttempts: default 0 (versionchanged 24.10.0, from 5); the source returns 5 (FusionConfig.DEFAULT_SNAPSHOT_MAX_SPOT_ATTEMPTS) when Fusion snapshots are on. aws.client.maxConnections: 'maximum number of open HTTP connections used by the S3 client' (default 50, S3SyncClientConfiguration line 35); this is the launcher JVM's client. workDir: 'Equivalent to the -work-dir option' (config.md line 33-34). It must be in S3 and contain at least one sub-directory (aws.md line 443; master mdx line 86). delayBetweenAttempts is 10 sec, retryMode is 'standard', terminateUnschedulableJobs exists since 25.04.0, logsGroup is /aws/batch/job. Config files may reference params (config.md shows `workflow.output.mode = params.publish_mode`). CORRECTION 1, region: the docs list aws.region, then AWS_REGION or AWS_DEFAULT_REGION, then EC2 metadata. The source is more specific: AwsConfig.getAwsRegion0 checks aws.region, then AWS_DEFAULT_REGION, then the profile region in ~/.aws/config; AwsClientFactory then falls back to AWS_REGION and then IMDS (InstanceProfileRegionProvider). CORRECTION 2, maxTransferAttempts: the docs say default 1, but AwsBatchConfig line 148 uses `opts.maxTransferAttempts ?: defaultMaxTransferAttempts()`, which is $AWS_MAX_ATTEMPTS or 5, and S3BashLib exports it to the task as AWS_MAX_ATTEMPTS. NEW CAVEAT: config.md says an included config 'is evaluated with the parameters defined before the include', so params.aws_queue, params.aws_region and params.container must be declared in nextflow.config before `includeConfig 'conf/awsbatch.config'`.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/reference/config.md (lines 33-34, 122-205) ; .../docs/aws.md (lines 32-36, 443) ; .../docs/config.md (lines 119-161) ; .../plugins/nf-amazon/src/main/nextflow/cloud/aws/config/AwsConfig.groovy (lines 134-160) ; .../config/AwsBatchConfig.groovy (lines 41, 145-161) ; .../cloud/aws/AwsClientFactory.groovy (lines 100-109) ; .../batch/AwsBatchTaskHandler.groovy (lines 807-814) ; .../nio/util/S3SyncClientConfiguration.java ; https://docs.aws.amazon.com/batch/latest/APIReference/API_SubmitJob.html
- **(3) Minimal launcher IAM policy, per Nextflow docs** (verified): CONFIRMED word for word against aws.md lines 40-100. Batch: CancelJob, DescribeComputeEnvironments, DescribeJobDefinitions, DescribeJobQueues, DescribeJobs, ListJobs, RegisterJobDefinition, SubmitJob, TagResource, TerminateJob. EC2/ECS: ec2:DescribeInstanceAttribute, DescribeInstances, DescribeInstanceStatus, DescribeInstanceTypes, ecs:DescribeContainerInstances, ecs:DescribeTasks. ECR: the 12 actions listed, 'Alternatively ... AmazonEC2ContainerRegistryReadOnly'. The AWS managed-policy reference (v3) shows that policy holds exactly those 12 ECR actions on Resource '*'. S3: s3:GetObject, PutObject, DeleteObject, PutObjectTagging, AbortMultipartUpload on arn:aws:s3:::<bucket>/*. The docs present the S3 part as a bucket policy (with a Principal) plus an AllowSSLRequestsOnly Deny statement; used as an identity policy, the Principal is dropped. Fargate-only extras (ec2:DescribeSubnets, ecs:CreateCluster etc.) are not needed here.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/aws.md (AWS IAM policies, S3 policies) ; https://docs.aws.amazon.com/aws-managed-policy/latest/reference/AmazonEC2ContainerRegistryReadOnly.html
- **(3) Launcher policy additions found in source (not in the docs list)** (likely): CONFIRMED in source at v26.04.7. S3Client calls listObjects (line 155), listObjectsV2Paginator (line 392), headBucket (line 371) and headObject, so s3:ListBucket on the bucket ARN is needed. S3FileSystemProvider.checkAccess (line 600 onward) calls getBucketAcl or getObjectAcl (lines 845-846) only when explicit AccessModes are given; with no modes (Files.exists) it only does an object-summary lookup. fetchCallerAccount (S3Client lines 129-139) calls ListBuckets(max 1) plus GetBucketAcl, and on failure it logs at debug and returns null. AwsBatchHelper.getLogStreamId and getTaskLogStream (lines 194-231) call batch:DescribeJobs and CloudWatch logs:GetLogEvents to fetch a failed job's log, so logs:GetLogEvents on /aws/batch/job is optional. iam:PassRole is needed only with aws.batch.jobRole or executionRole. Still unverified whether the launcher actually needs the ECR actions, since the launcher never pulls images.  
  Source: https://github.com/nextflow-io/nextflow/blob/v26.04.7/plugins/nf-amazon/src/main/nextflow/cloud/aws/nio/S3Client.java ; .../nio/S3FileSystemProvider.java ; .../batch/AwsBatchHelper.groovy
- **Instance role managed policies** (verified): CONFIRMED. arn:aws:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role is Type 'Service role policy', version v10, edited February 12, 2026 17:59 UTC; note the service-role/ path. It grants ecr:GetAuthorizationToken, BatchCheckLayerAvailability, GetDownloadUrlForLayer, BatchGetImage and logs:CreateLogStream and PutLogEvents, plus the ECS agent actions. arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly (v3, edited 2019-12-10) adds the Describe/List/Get*Policy actions. The Nextflow docs (aws.md lines 393 and 397) say to attach both to the instance role. The docs' quick-start (line 182) also suggests AmazonS3FullAccess on the instance role; the sketch deliberately uses a bucket-scoped inline policy instead.  
  Source: https://docs.aws.amazon.com/aws-managed-policy/latest/reference/AmazonEC2ContainerServiceforEC2Role.html ; https://docs.aws.amazon.com/aws-managed-policy/latest/reference/AmazonEC2ContainerRegistryReadOnly.html ; https://github.com/nextflow-io/nextflow/blob/v26.04.7/docs/aws.md
- **Launch template root volume on AL2023 ECS AMIs (/dev/xvda)** (verified): CONFIRMED. The Batch launch-template page's example 'resizes the Docker data volume for the default AWS Batch compute resource AMI' with DeviceName /dev/xvda, Ebs {Encrypted true, VolumeSize 100, VolumeType gp2}. API_Ec2Configuration says: 'If Ec2Configuration isn't specified, the default is ECS_AL2023'. The ECS AMI page says the AL2023 ECS AMI 'ships with a single 30-GiB root volume', which can be enlarged at launch and holds the OS and 'Docker images and metadata' (xfs, overlay2). aws/amazon-ecs-ami al2023.pkr.hcl (main; last commit to that file 2026-08-05) has launch_block_device_mappings with device_name '/dev/xvda', volume_type 'gp3', volume_size var.block_device_size_gb, and variables.pkr.hcl sets the default to 30. Batch ignores these LT fields: instance type, instance role, network interface subnets, instance market options, disable API termination. CE values take precedence over the LT for key pair, AMI ID and security group IDs; EC2 tags are merged, and the CE wins on key collisions. User data must be MIME multi-part.  
  Source: https://docs.aws.amazon.com/batch/latest/userguide/launch-templates.html ; https://docs.aws.amazon.com/batch/latest/APIReference/API_Ec2Configuration.html ; https://docs.aws.amazon.com/AmazonECS/latest/developerguide/ecs-optimized_AMI.html ; https://github.com/aws/amazon-ecs-ami/blob/main/al2023.pkr.hcl + variables.pkr.hcl
- **Launch template version resolution** (verified): CONFIRMED. Batch does not attach your LT to the Auto Scaling group. It creates its own Batch-managed LT from your LT's settings and 'only updates the launch template with a new launch template version during infrastructure updates', so $Default/$Latest are picked up at create time or at an infrastructure update, not at each instance launch. LaunchTemplateSpecification: 'During an infrastructure update, if either $Default or $Latest is specified, AWS Batch re-evaluates the launch template version ... even if the launch template isn't specified in the update.' The CFN CE page says the same for ReplaceComputeEnvironment false. In spec v269.0.0, AWS::EC2::LaunchTemplate has the attributes LatestVersionNumber, LaunchTemplateId and DefaultVersionNumber, so Version: !GetAtt LaunchTemplate.LatestVersionNumber is valid, and changing the LT changes the CE property, which triggers an infrastructure update.  
  Source: https://docs.aws.amazon.com/batch/latest/userguide/launch-templates.html ; https://docs.aws.amazon.com/batch/latest/APIReference/API_LaunchTemplateSpecification.html ; https://docs.aws.amazon.com/AWSCloudFormation/latest/TemplateReference/aws-resource-batch-computeenvironment.html ; CloudFormationResourceSpecification.json v269.0.0
- **IMDS settings in the launch template** (likely): UPDATED. Every amazon-ecs-ami recipe, including al2023.pkr.hcl, sets metadata_options (http_tokens required, hop limit 2) only for the packer build instance; no .hcl file in the repo sets imds_support. AWS Batch's own 'Instance Metadata Service (IMDS) configuration' page tells users to 'add metadata options to your launch template to require IMDSv2' and gives the example LaunchTemplateData.MetadataOptions.HttpTokens 'required'. So Batch honouring MetadataOptions is documented, not just inferred from the ignored-fields list. The same page says 'Amazon Linux 2023 requires IMDSv2 by default', which conflicts with the recipe not setting imds_support; the explicit LT setting makes the result independent of both. EC2 docs: 'if the hop limit is 1 in a container environment, the call might not receive a response at all because going to the container is considered an additional network hop ... consider increasing the hop limit to 2.' With no jobRole, the task's aws CLI gets instance-role credentials through IMDS, so HttpTokens required plus HttpPutResponseHopLimit 2 is the right pair. Not tested on a live Batch instance.  
  Source: https://docs.aws.amazon.com/batch/latest/userguide/imds-compute-environments.html ; https://github.com/aws/amazon-ecs-ami (all *.pkr.hcl on main) ; https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/instancedata-data-retrieval.html ; https://docs.aws.amazon.com/batch/latest/userguide/launch-templates.html
- **Compute environment: allocation strategy, instance types, Spot role** (verified): CONFIRMED. SPOT_PRICE_CAPACITY_OPTIMIZED: 'only available for Spot Instance compute resources'. BEST_FIT_PROGRESSIVE is valid for EC2. BEST_FIT is the default for ECS when unset, and 'Compute resources that use a BEST_FIT allocation strategy don't support infrastructure updates'. 'optimal': 'Semantically equivalent to default_x86_64' (m6i, c6i, r6i, c7i). default_arm64 is m6g, c6g, r6g, c7g. Instance types 'must share the same architecture'. spotIamFleetRole: 'required if the allocation strategy set to BEST_FIT or if the allocation strategy isn't specified'. The `type` description on the same page says SPOT requires spotIamFleetRole, so the page contradicts itself; the allocation-strategies page agrees with the BEST_FIT-only reading. Valid values now also include BEST_FIT_PROGRESSIVE_ORDERED (On-Demand only) and SPOT_CAPACITY_OPTIMIZED_PRIORITIZED.  
  Source: https://docs.aws.amazon.com/batch/latest/APIReference/API_ComputeResource.html ; https://docs.aws.amazon.com/batch/latest/userguide/allocation-strategies.html
- **Batch service role is optional** (verified): CONFIRMED. ServiceRole is Required: No. 'If your account already created the AWS Batch service-linked role, that role is used by default ... If the AWS Batch service-linked role doesn't exist in your account, and no role is specified here, the service attempts to create' it; automatic creation applies to MANAGED CEs only. Infrastructure updates 'require the AWSServiceRoleForBatch service-linked role and an allocation strategy of BEST_FIT_PROGRESSIVE, BEST_FIT_PROGRESSIVE_ORDERED, SPOT_CAPACITY_OPTIMIZED, SPOT_PRICE_CAPACITY_OPTIMIZED or SPOT_CAPACITY_OPTIMIZED_PRIORITIZED'. ReplaceComputeEnvironment: 'The default value is true ... When changing the value of this property to false, do not change any other properties at the same time' (risk of UPDATE_ROLLBACK_FAILED).  
  Source: https://docs.aws.amazon.com/AWSCloudFormation/latest/TemplateReference/aws-resource-batch-computeenvironment.html ; https://docs.aws.amazon.com/batch/latest/APIReference/API_CreateComputeEnvironment.html ; https://docs.aws.amazon.com/batch/latest/userguide/infrastructure-updates.html
- **Required properties (CFN resource spec v269.0.0, Last-Modified Thu, 22 Oct 2026 21:34:30 GMT)** (verified): CONFIRMED from the spec JSON. Required properties: AWS::Batch::ComputeEnvironment: Type. ComputeResources: MaxvCpus and Type. The API adds instanceRole ('required for Amazon EC2 instances types') and subnets ('required for ... EC2, SPOT, FARGATE, or FARGATE_SPOT'). AWS::Batch::JobQueue: Priority only; ComputeEnvironmentOrder is 'Required: No' in the CFN docs, with ServiceEnvironmentOrder as the alternative, and each ComputeEnvironmentOrder entry requires ComputeEnvironment and Order. AWS::EC2::LaunchTemplate: LaunchTemplateData. AWS::ECR::Repository: none (EmptyOnDelete exists). AWS::S3::Bucket: none. S3 Rule requires Status, and its properties include Prefix and TagFilters but no Filter, both in the spec and in the CFN Rule docs page. AWS::IAM::Role: AssumeRolePolicyDocument; Policy entries require PolicyName and PolicyDocument. AWS::IAM::InstanceProfile: Roles. Ec2ConfigurationObject requires ImageType. Ref of JobQueue returns the ARN, Ref of ComputeEnvironment returns the ARN, Ref of LaunchTemplate returns the ID (per each CFN docs page).  
  Source: https://d1uauaxba7bl26.cloudfront.net/latest/gzip/CloudFormationResourceSpecification.json (ResourceSpecificationVersion 269.0.0) ; https://docs.aws.amazon.com/AWSCloudFormation/latest/TemplateReference/aws-resource-batch-jobqueue.html ; .../aws-resource-ec2-launchtemplate.html ; .../aws-properties-s3-bucket-rule.html ; https://docs.aws.amazon.com/batch/latest/APIReference/API_ComputeResource.html
- **Template validation done** (verified): RE-RUN INDEPENDENTLY, with one CORRECTION. I converted the sketch with my own Ruby/Psych converter (short-form tags to Fn:: maps) and checked it with my own Python checker against spec v269.0.0. The checker covers resource types, property names, nested property types, list/map/primitive shapes, required properties, Ref/GetAtt/Sub targets, GetAtt attributes and condition names. Result: 0 errors. A deliberately broken copy (Filter in an S3 Rule, missing JobQueue Priority, missing MaxvCpus, bad GetAtt LatestVersion) gave exactly those 4 errors. The scratchpad file is byte-identical in content to the snippet. CORRECTION: the colleague said flow sequences like `Action: [s3:ListBucket, s3:GetBucketLocation]` fail in libyaml and that PyYAML 'applies the same rule'. That is only true of old libyaml. The failure reproduces here because macOS's system Ruby 2.6 uses Psych 3.1.0 with libyaml 0.2.1, whose scanner rejects any ':' followed by a non-blank in flow context. libyaml 0.2.2 and later (0.2.5 is what PyYAML 6.0.3 wheels build with) reject only ':' followed by , ? [ ] { }. PyYAML 6.0.3's pure-Python scanner allows ':' followed by anything except a blank or a flow indicator. cfn-lint 1.57.2 requires pyyaml>=6.0.3 and uses CSafeLoader when available, else SafeLoader. So cfn-lint would accept the unquoted flow form. Block lists, as in the sketch, are still the portable choice. What CloudFormation's own parser does with that form was not checked. cfn-lint itself was NOT run.  
  Source: /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/verify-aws-tools/y2j.rb ; /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/verify-aws-tools/check.py ; /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/research/verify-aws/cfnspec/spec.json ; https://github.com/yaml/libyaml/blob/0.2.1/src/scanner.c vs .../0.2.2/src/scanner.c and .../0.2.5/src/scanner.c (yaml_parser_scan_plain_scalar) ; https://github.com/yaml/pyyaml/blob/6.0.3/lib/yaml/scanner.py (lines 1285-1297) ; https://github.com/yaml/pyyaml/blob/6.0.3/.github/workflows/ci.yaml (LIBYAML_REF 0.2.5) ; https://github.com/aws-cloudformation/cfn-lint/blob/v1.57.2/src/cfnlint/decode/cfn_yaml.py
- **cfn-lint** (verified): CONFIRMED, with a CORRECTION on the local environment. The latest release, 1.58.0 (PyPI 2026-10-20T23:48:54Z; conda-forge 2026-10-21), is too new. Use 1.57.2: PyPI wheel uploaded 2026-10-06T19:29:13Z, GitHub release v1.57.2 published 2026-10-06T17:55:20Z (not a prerelease), conda-forge noarch build cfn-lint-1.57.2-pyh5ded981_0 uploaded 2026-10-06 22:55 UTC. PyPI requires_python is '<3.15,>=3.10' (README: 'Python 3.10 to 3.14 are supported'), and it requires pyyaml>=6.0.3. The conda-forge build depends on python >=3.11, not 3.10. Usage `cfn-lint template.yaml` is in the README. CORRECTION: /usr/bin/python3 is 3.9.6 and is first on PATH, but /opt/homebrew/bin/python3 is Python 3.14.8, which is inside cfn-lint's supported range. So cfn-lint could run here from a venv built on Homebrew Python. I did not do that, because installing was out of scope. pipeline/environment.yml with conda-forge cfn-lint=1.57.2 remains a good place to pin it.  
  Source: https://pypi.org/pypi/cfn-lint/json ; https://pypi.org/pypi/cfn-lint/1.57.2/json ; https://api.github.com/repos/aws-cloudformation/cfn-lint/releases/tags/v1.57.2 ; https://api.anaconda.org/package/conda-forge/cfn-lint ; https://github.com/aws-cloudformation/cfn-lint/blob/v1.57.2/README.md ; local `python3 --version` checks
- **S3 cross-account reads need IAM permission** (verified): CONFIRMED. The S3 object-operation authorization page says, on User context: 'If the requester is an IAM principal, the principal must have permission from the parent AWS account to which it belongs.' A task whose instance-role policy covers only the work bucket would be denied on a signed `aws s3 cp s3://gnomad-public-us-east-1/...`. Fixes: stream over HTTPS with curl, use the global --no-sign-request option (present in the 2.37.12 CLI's data/cli.json options), or allow s3:GetObject and ListBucket on the gnomAD bucket. The sketch does the last one, as an optional statement.  
  Source: https://docs.aws.amazon.com/AmazonS3/latest/userguide/access-control-auth-workflow-object-operation.html ; aws/dist/awscli/data/cli.json inside awscli-exe-linux-x86_64-2.37.12.zip
- **gnomAD on AWS Open Data: region and anonymous read** (verified): CONFIRMED by re-running it on 2026-10-24 at 20:35 UTC. The registry YAML has ARN arn:aws:s3:::gnomad-public-us-east-1, Region us-east-1. An anonymous ListObjectsV2 returned HTTP 200 with x-amz-bucket-region: us-east-1. An anonymous HEAD on https://gnomad-public-us-east-1.s3.amazonaws.com/release/4.1.1/vcf/genomes/gnomad.genomes.v4.1.1.sites.chr21.vcf.bgz returned 200: Content-Length 7796525262, Last-Modified Thu, 26 Mar 2026 20:31:29 GMT, ETag "46456dc66dd159eb32cc7257a137ecde-465" (multipart, so not an MD5), x-amz-storage-class INTELLIGENT_TIERING. GCS JSON metadata for the same object: size 7796525262, md5Hash vQiPhuGSSpcCxbpdxeFgUw==.  
  Source: curl -I (2026-10-24 20:35 UTC) ; https://raw.githubusercontent.com/awslabs/open-data-registry/main/datasets/broad-gnomad.yaml ; https://storage.googleapis.com/storage/v1/b/gcp-public-data--gnomad/o/release%2F4.1.1%2Fvcf%2Fgenomes%2Fgnomad.genomes.v4.1.1.sites.chr21.vcf.bgz
- **gnomAD 'latest' resolution: 4.1.2 exists but has no VCFs** (verified): CONFIRMED. A delimiter listing of release/ on both mirrors gives 4.0/, 4.1/, 4.1.1/ and 4.1.2/. Under release/4.1.2/, a delimiter listing shows only constraint/. prefix=release/4.1.2/vcf/ returns KeyCount 0 on S3 and no items on GCS. Object dates: S3 LastModified 2026-09-23T16:20Z, GCS timeCreated 2026-09-23T00:01Z. release/4.1.1/vcf/ holds exomes/ and genomes/. RESOLVE_GNOMAD with `latest` must choose the newest release that has vcf/genomes (or vcf/exomes) objects, which today is 4.1.1.  
  Source: https://gnomad-public-us-east-1.s3.amazonaws.com/?list-type=2&prefix=release/4.1.2/&delimiter=/ ; ...&prefix=release/4.1.2/vcf/ ; https://storage.googleapis.com/storage/v1/b/gcp-public-data--gnomad/o?prefix=release/4.1.2/&delimiter=/ ; ...?prefix=release/4.1.2/vcf/
- **Deploy and teardown notes for docs/aws-batch.md** (verified): NOW VERIFIED; the colleague had not re-fetched these. CreateStack API: 'If you have IAM resources, you can specify either capability. If you have IAM resources with custom names, you must specify CAPABILITY_NAMED_IAM.' The `aws cloudformation deploy` reference repeats this and documents --template-file, --stack-name, --parameter-overrides (Key=Value pairs) and --capabilities. The template sets no RoleName or InstanceProfileName; PolicyName is the name of an inline policy inside the role. So --capabilities CAPABILITY_IAM is enough. The bucket has DeletionPolicy and UpdateReplacePolicy Retain, so it survives stack deletion and must be emptied and deleted by hand. ECR EmptyOnDelete: 'If true, deleting the repository force deletes the contents of the repository.' Design consequence (derived, not a doc fact): the work/ lifecycle rule expires objects after WorkExpirationDays (default 30), so `-resume` more than 30 days later loses the cached task outputs.  
  Source: https://docs.aws.amazon.com/AWSCloudFormation/latest/APIReference/API_CreateStack.html ; https://docs.aws.amazon.com/cli/latest/reference/cloudformation/deploy.html ; https://docs.aws.amazon.com/AWSCloudFormation/latest/TemplateReference/aws-resource-ecr-repository.html

### Open questions the researchers raised

- cfn-lint was still not run, because nothing may be installed. Unlike the earlier report, this machine does have a Python cfn-lint supports (/opt/homebrew/bin/python3 is 3.14.8; /usr/bin/python3 is 3.9.6). A venv plus `pip install cfn-lint==1.57.2` would run it. My checks cover property names, nested types, required properties and Ref/GetAtt/Sub/condition targets against spec v269.0.0. They do not cover cfn-lint's allowed-value, IAM-schema or best-practice rules.
- How CloudFormation's own YAML parser treats unquoted flow sequences such as `[s3:ListBucket, s3:GetObject]` was not checked. libyaml 0.2.2+ and PyYAML 6.0.3, as used by cfn-lint, accept them, and only the old libyaml 0.2.1 in macOS's system Ruby rejects them. Block lists, as used in the sketch, avoid the question.
- Spot Fleet role: the API reference still contradicts itself. The `type` text says SPOT requires spotIamFleetRole; the spotIamFleetRole text and the allocation-strategies page say it is needed only for BEST_FIT or no strategy. Batch's 'Amazon EC2 spot fleet role' page also says Spot Fleet compute environments need the AWSServiceRoleForEC2Spot and AWSServiceRoleForEC2SpotFleet service-linked roles. Whether that applies to SPOT_PRICE_CAPACITY_OPTIMIZED, and whether those roles must already exist in a fresh account, is unverified without a deployment.
- Does the Nextflow launcher need the ECR actions the docs list? Unverified, since it never pulls images; they are kept to match the docs.
- EBS Encrypted: true uses the account's default EBS key. If that default is a customer-managed KMS key, the Auto Scaling or Batch service-linked roles may need key-policy grants. Not verified.
- IMDS: Batch's IMDS page says 'Amazon Linux 2023 requires IMDSv2 by default', but no amazon-ecs-ami recipe sets imds_support. The sketch's explicit MetadataOptions (HttpTokens required, hop limit 2) makes this moot, but it has not been tested on a live Batch instance.
- aws.batch.maxTransferAttempts: the v26.04.7 docs say the default is 1, but the code uses $AWS_MAX_ATTEMPTS or 5. If the plan relies on a specific retry count, set it explicitly.
- The GPG signatures of the 2.37.12 installers (.zip.sig) were not verified, because no OpenPGP tool is installed. Only the SHA-256 values computed from HTTPS downloads were checked.
- gnomAD `latest` must pick the newest release that has vcf/ objects (today 4.1.1). release/4.1.2/ exists on both mirrors (created 2026-09-23) but holds only constraint/.
- debian:bookworm-slim on Docker Hub was rebuilt 2026-10-20T21:32:55Z (amd64 config), less than two weeks ago. Pinning by digest under the two-week rule needs a build from 2026-10-10 or earlier; that was not researched here.
- The work/ lifecycle expiry (default 30 days) means a `-resume` more than 30 days after a run loses the cache. The plan should state this or raise WorkExpirationDays.

### Snippets

#### pipeline/aws/clingen-batch.yaml (sketch; re-validated independently against CFN spec v269.0.0: 0 errors; unchanged; also at /private/tmp/claude-501/-Users-leipzig-Documents-dev/4c3cb95f-8eee-416a-a136-1c106dbe2b92/scratchpad/cfn/clingen-batch.yaml)

```
AWSTemplateFormatVersion: "2010-09-09"
Description: >-
  AWS Batch setup for the ClinGen CA-ID index regeneration pipeline (Nextflow awsbatch profile):
  S3 bucket (work/ expires), ECR repository, ECS instance role and profile, launch template with a
  larger gp3 root volume, managed EC2/Spot compute environment and a job queue.

Parameters:
  BucketName:
    Type: String
    Default: ""
    Description: Name for the work/results bucket; leave empty to let CloudFormation generate one.
  RepositoryName:
    Type: String
    Default: clingen-index
  QueueName:
    Type: String
    Default: clingen
  ComputeType:
    Type: String
    Default: SPOT
    AllowedValues: [SPOT, EC2]
  InstanceTypes:
    Type: CommaDelimitedList
    Default: optimal
    Description: "'optimal' (= default_x86_64: m6i, c6i, r6i, c7i), 'default_arm64', or families such as c6i,m6i,r6i,c7i,m7i"
  MaxvCpus:
    Type: Number
    Default: 64
    MinValue: 1
  RootVolumeGiB:
    Type: Number
    Default: 200
    MinValue: 30
    Description: Root (and Docker) volume of each instance; the ECS AL2023 AMI default is 30 GiB.
  WorkExpirationDays:
    Type: Number
    Default: 30
    MinValue: 1
  Subnets:
    Type: List<AWS::EC2::Subnet::Id>
    Description: Subnets with internet egress (NAT or public IP) for the Registry, NCBI, UCSC and GCS.
  SecurityGroupIds:
    Type: List<AWS::EC2::SecurityGroup::Id>
    Description: Security groups for the instances; outbound HTTPS is enough, no inbound rules needed.

Conditions:
  IsSpot: !Equals [!Ref ComputeType, SPOT]
  HasBucketName: !Not [!Equals [!Ref BucketName, ""]]

Resources:
  Bucket:
    Type: AWS::S3::Bucket
    DeletionPolicy: Retain
    UpdateReplacePolicy: Retain
    Properties:
      BucketName: !If [HasBucketName, !Ref BucketName, !Ref "AWS::NoValue"]
      OwnershipControls:
        Rules:
          - ObjectOwnership: BucketOwnerEnforced
      PublicAccessBlockConfiguration:
        BlockPublicAcls: true
        BlockPublicPolicy: true
        IgnorePublicAcls: true
        RestrictPublicBuckets: true
      BucketEncryption:
        ServerSideEncryptionConfiguration:
          - ServerSideEncryptionByDefault:
              SSEAlgorithm: AES256
      LifecycleConfiguration:
        Rules:
          - Id: expire-work
            Status: Enabled
            Prefix: work/
            ExpirationInDays: !Ref WorkExpirationDays
            AbortIncompleteMultipartUpload:
              DaysAfterInitiation: 7

  Repository:
    Type: AWS::ECR::Repository
    Properties:
      RepositoryName: !Ref RepositoryName
      ImageTagMutability: MUTABLE
      ImageScanningConfiguration:
        ScanOnPush: true
      EmptyOnDelete: true
      LifecyclePolicy:
        LifecyclePolicyText: |
          {"rules":[{"rulePriority":1,"description":"expire untagged images after 14 days",
            "selection":{"tagStatus":"untagged","countType":"sinceImagePushed","countUnit":"days","countNumber":14},
            "action":{"type":"expire"}}]}

  InstanceRole:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Allow
            Principal:
              Service: ec2.amazonaws.com
            Action: sts:AssumeRole
      ManagedPolicyArns:
        - !Sub arn:${AWS::Partition}:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role
        - !Sub arn:${AWS::Partition}:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly
      Policies:
        - PolicyName: clingen-bucket
          PolicyDocument:
            Version: "2012-10-17"
            Statement:
              - Sid: ListBucket
                Effect: Allow
                Action:
                  - s3:ListBucket
                  - s3:GetBucketLocation
                Resource: !GetAtt Bucket.Arn
              - Sid: Objects
                Effect: Allow
                Action:
                  - s3:GetObject
                  - s3:PutObject
                  - s3:DeleteObject
                  - s3:PutObjectTagging
                  - s3:AbortMultipartUpload
                Resource: !Sub "${Bucket.Arn}/*"
              # Only needed if tasks read gnomAD as s3:// inputs with signed requests;
              # HTTPS (curl) or `aws s3 --no-sign-request` needs no IAM permission.
              - Sid: GnomadPublicRead
                Effect: Allow
                Action:
                  - s3:GetObject
                  - s3:ListBucket
                Resource:
                  - arn:aws:s3:::gnomad-public-us-east-1
                  - arn:aws:s3:::gnomad-public-us-east-1/*

  InstanceProfile:
    Type: AWS::IAM::InstanceProfile
    Properties:
      Roles:
        - !Ref InstanceRole

  LaunchTemplate:
    Type: AWS::EC2::LaunchTemplate
    Properties:
      LaunchTemplateData:
        # No ImageId: Batch picks the ECS-optimized AL2023 AMI (root device /dev/xvda).
        BlockDeviceMappings:
          - DeviceName: /dev/xvda
            Ebs:
              VolumeSize: !Ref RootVolumeGiB
              VolumeType: gp3
              Encrypted: true
              DeleteOnTermination: true
        MetadataOptions:
          HttpEndpoint: enabled
          HttpTokens: required
          HttpPutResponseHopLimit: 2

  ComputeEnvironment:
    Type: AWS::Batch::ComputeEnvironment
    Properties:
      Type: MANAGED
      State: ENABLED
      # No ServiceRole: Batch uses (or creates) the AWSServiceRoleForBatch service-linked role.
      ReplaceComputeEnvironment: false
      UpdatePolicy:
        TerminateJobsOnUpdate: false
        JobExecutionTimeoutMinutes: 30
      ComputeResources:
        Type: !Ref ComputeType
        AllocationStrategy: !If [IsSpot, SPOT_PRICE_CAPACITY_OPTIMIZED, BEST_FIT_PROGRESSIVE]
        MinvCpus: 0
        MaxvCpus: !Ref MaxvCpus
        InstanceTypes: !Ref InstanceTypes
        Subnets: !Ref Subnets
        SecurityGroupIds: !Ref SecurityGroupIds
        InstanceRole: !GetAtt InstanceProfile.Arn
        Ec2Configuration:
          - ImageType: ECS_AL2023
        LaunchTemplate:
          LaunchTemplateId: !Ref LaunchTemplate
          Version: !GetAtt LaunchTemplate.LatestVersionNumber
        UpdateToLatestImageVersion: true
        Tags:
          Name: clingen-batch

  JobQueue:
    Type: AWS::Batch::JobQueue
    Properties:
      JobQueueName: !Ref QueueName
      State: ENABLED
      Priority: 1
      ComputeEnvironmentOrder:
        - Order: 1
          ComputeEnvironment: !Ref ComputeEnvironment

Outputs:
  BucketName:
    Value: !Ref Bucket
  WorkDir:
    Description: Pass as -work-dir (must be a sub-path, not the bucket root)
    Value: !Sub "s3://${Bucket}/work"
  ResultsPrefix:
    Value: !Sub "s3://${Bucket}/results"
  JobQueueName:
    Description: Pass as --aws_queue (process.queue)
    Value: !Ref QueueName
  JobQueueArn:
    Value: !GetAtt JobQueue.JobQueueArn
  RepositoryUri:
    Description: Image is <RepositoryUri>:<tag>
    Value: !GetAtt Repository.RepositoryUri
  ComputeEnvironmentArn:
    Value: !Ref ComputeEnvironment
  InstanceRoleArn:
    Value: !GetAtt InstanceRole.Arn
```

#### pipeline/conf/awsbatch.config (keys per Nextflow v26.04.7 docs and source; added the include-order caveat)

```
// params.aws_queue, params.aws_region and params.container must be declared in nextflow.config
// BEFORE `includeConfig 'conf/awsbatch.config'`: an included config only sees params defined before the include.
process {
    executor  = 'awsbatch'
    queue     = params.aws_queue      // job queue name or ARN (stack output JobQueueName)
    container = params.container      // <account>.dkr.ecr.<region>.amazonaws.com/clingen-index:0.1.0
}
aws {
    region = params.aws_region        // default 'us-east-1'; Nextflow also passes it to the task CLI as `aws --region ...`
    batch {
        // no cliPath: AWS CLI v2 is in the image at /usr/local/bin/aws and found on PATH.
        // Never set cliPath = '/usr/local/bin/aws': Nextflow would bind-mount the HOST's /usr over the container's.
        maxParallelTransfers = 4      // per job (default 4)
        maxSpotAttempts      = 3      // default 0 since 24.10.0; >0 lets Batch retry Spot reclaims
        // maxTransferAttempts: docs say default 1, but v26.04.7 code defaults to $AWS_MAX_ATTEMPTS or 5
    }
    client {
        maxConnections = 50           // launcher-side S3 client (default 50)
    }
}
// workDir must be in S3 and include a sub-directory; pass -work-dir s3://<bucket>/work on the command line
// (or: workDir = 's3://<bucket>/work')
```

#### Launcher IAM policy (Nextflow docs list + s3:ListBucket and logs:GetLogEvents found in source; unchanged)

```
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "Batch", "Effect": "Allow", "Resource": "*", "Action": [
      "batch:CancelJob", "batch:DescribeComputeEnvironments", "batch:DescribeJobDefinitions",
      "batch:DescribeJobQueues", "batch:DescribeJobs", "batch:ListJobs", "batch:RegisterJobDefinition",
      "batch:SubmitJob", "batch:TagResource", "batch:TerminateJob"]},
    {"Sid": "Ec2EcsDescribe", "Effect": "Allow", "Resource": "*", "Action": [
      "ec2:DescribeInstanceAttribute", "ec2:DescribeInstances", "ec2:DescribeInstanceStatus",
      "ec2:DescribeInstanceTypes", "ecs:DescribeContainerInstances", "ecs:DescribeTasks"]},
    {"Sid": "EcrReadPerNextflowDocs", "Effect": "Allow", "Resource": "*", "Action": [
      "ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:DescribeImages", "ecr:DescribeImageScanFindings",
      "ecr:DescribeRepositories", "ecr:GetAuthorizationToken", "ecr:GetDownloadUrlForLayer", "ecr:GetLifecyclePolicy",
      "ecr:GetLifecyclePolicyPreview", "ecr:GetRepositoryPolicy", "ecr:ListImages", "ecr:ListTagsForResource"]},
    {"Sid": "WorkBucket", "Effect": "Allow", "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>"},
    {"Sid": "WorkObjects", "Effect": "Allow", "Action": [
      "s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:PutObjectTagging", "s3:AbortMultipartUpload"],
      "Resource": "arn:aws:s3:::<bucket>/*"},
    {"Sid": "FailedJobLogs", "Effect": "Allow", "Action": ["logs:GetLogEvents"],
      "Resource": "arn:aws:logs:<region>:<account>:log-group:/aws/batch/job:*"}
  ]
}
```

#### Dockerfile fragment: pinned AWS CLI v2 2.37.12 (SHA-256s re-computed from fresh HTTPS downloads and matching; GPG signature not verified)

```
# runtime stage (debian:bookworm-slim); BuildKit sets TARGETARCH (declare it in the stage)
ARG TARGETARCH
ARG AWSCLI_VERSION=2.37.12
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl unzip; \
    case "${TARGETARCH}" in \
      amd64) A=x86_64;  SHA=e6d3c672fb36ed4b12c354c39a993eee2b34da1437cafb10bac4480840cfa83f ;; \
      arm64) A=aarch64; SHA=829c76096e05f743267d75098bc7cc7cfae799fc7e568554ebaa6d78e6aba753 ;; \
      *) echo "unsupported arch ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/awscliv2.zip "https://awscli.amazonaws.com/awscli-exe-linux-${A}-${AWSCLI_VERSION}.zip"; \
    echo "${SHA}  /tmp/awscliv2.zip" | sha256sum -c -; \
    unzip -q /tmp/awscliv2.zip -d /tmp; \
    /tmp/aws/install --install-dir /usr/local/aws-cli --bin-dir /usr/local/bin; \
    rm -rf /tmp/aws /tmp/awscliv2.zip; \
    apt-get purge -y --auto-remove unzip; \
    rm -rf /var/lib/apt/lists/*; \
    aws --version
# groff and less (listed by AWS as CLI prerequisites) are only used by `aws help` and the interactive pager;
# non-interactive `aws s3 cp` as run by Nextflow does not need them.
# no ENTRYPOINT (Nextflow runs: bash -o pipefail -c "aws --region <r> s3 cp .../.command.run - | bash ...")
```

#### Deploy and lint commands for docs/aws-batch.md (added --region to describe-stacks so it queries the stack's region)

```
# lint (Python 3.10-3.14 per cfn-lint README; PyPI requires_python >=3.10,<3.15; the conda-forge build needs python >=3.11)
# pin in environment.yml as conda-forge cfn-lint=1.57.2
cfn-lint pipeline/aws/clingen-batch.yaml

aws cloudformation deploy --region us-east-1 --stack-name clingen-batch \
  --template-file pipeline/aws/clingen-batch.yaml --capabilities CAPABILITY_IAM \
  --parameter-overrides Subnets=subnet-aaa,subnet-bbb SecurityGroupIds=sg-xxx ComputeType=SPOT MaxvCpus=64 RootVolumeGiB=200
aws cloudformation describe-stacks --region us-east-1 --stack-name clingen-batch --query 'Stacks[0].Outputs'
```

