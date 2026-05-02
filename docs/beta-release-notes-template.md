# AppRelay Beta Release Notes Template

Release runner:
Commit SHA:
Release date:
Artifact set:

This source-built or local limited beta note does not claim public beta
readiness, production support, signed native package availability, or automatic
telemetry.

## Known Limitations

- Supported platforms for this beta: <required: list every included server and client platform>
- Unsupported platforms for this beta: <required: list excluded or explicitly unsupported platforms; explicitly state that Windows desktop-server workflows are excluded or unsupported until a separate Windows application discovery/launch implementation and evidence gate exists>
- Unsupported or partial features: <required: include pairing UI/device verification, native media/input gaps, Windows desktop-server discovery/launch exclusion if unsupported, and typed unsupported paths>
- Artifact signing and distribution status: <required: state signed, unsigned manual-runner, source-built, or blocked for each artifact>
- Dependency audit status: <required: include Node beta audit and Rust Advisories evidence, or state blocked>
- Install, upgrade, uninstall, and rollback status: <required: state package-manager evidence or manual generated-plan boundary>
- Local network and tunnel boundary: <required: state loopback/trusted-LAN/SSH boundary and broad exposure prohibition>
- Native package gaps: <required: list signing, notarization, repository metadata, mobile distribution, or package gaps>
- Security and privacy limitations: <required: include manual telemetry-free diagnostics, file-backed tokens, audit retention limits, and no-secret feedback rules>
- Feedback and crash reporting channel: <required: name the private beta channel and manual crash evidence path>

Known limitations cannot waive blockers from the threat model, dependency audit
policy, signed artifact policy, or local network guidance.

## Release Evidence

- CI run:
- Dependency audit record:
- Signed artifact or checksum record:
- Install/rollback evidence:

## Feedback Channel

Name the private channel for this beta round and repeat that testers must not
attach secrets, raw logs, media contents, keyboard input, or unredacted private
network details.
