import { describe, expect, it } from "vitest";
import { validateCiWorkflow } from "./check-ci-workflow.mjs";

const validWorkflow = `name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm
    steps:
      - name: Check CI workflow policy
        run: node apps/client-tauri/scripts/check-ci-workflow.mjs

  rust:
    name: Rust
    needs:
      - ci-policy
    runs-on:
      - self-hosted
      - linux
      - docker
    container:
      image: rust:1.86-bookworm
    steps:
      - name: Format
        run: cargo fmt --all --check
      - name: Lint
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test
        run: cargo test --workspace --locked

  tauri-rust:
    name: Tauri Rust
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: rust:1.86-bookworm
    steps:
      - name: Check
        run: cargo check --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked
      - name: Test
        run: cargo test --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked

  rust-advisories:
    name: Rust Advisories
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: rust:1.86-bookworm
    steps:
      - name: Install cargo-audit
        run: cargo install cargo-audit --version "$CARGO_AUDIT_VERSION" --locked
      - name: Verify workspace lockfile
        run: cargo metadata --locked --format-version 1 > /dev/null
      - name: Audit workspace lockfile
        run: cargo audit --file Cargo.lock
      - name: Verify Tauri lockfile
        run: cargo metadata --manifest-path apps/client-tauri/src-tauri/Cargo.toml --locked --format-version 1 > /dev/null
      - name: Audit Tauri lockfile
        run: cargo audit --file apps/client-tauri/src-tauri/Cargo.lock

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm
    steps:
      - name: Install
        run: npm ci
      - name: Audit beta dependencies
        run: npm run audit:beta
      - name: Test mobile contract
        run: npm run mobile-contract:test
      - name: Test
        run: npm run test:ci
      - name: Build
        run: npm run build
      - name: Check packaging config
        run: npm run package:check
      - name: Check release artifact manifest template
        run: npm run release-artifacts:check
      - name: Check dependency audit evidence manifest template
        run: npm run dependency-audit-evidence:check
      - name: Check lifecycle evidence manifest template
        run: npm run lifecycle-evidence:check
      - name: Check beta release notes template
        run: npm run release-notes:check
      - name: Check beta security review manifest template
        run: npm run beta-security-review:check
`;

describe("ci workflow checker", () => {
  it("accepts jobs with the required Docker runner labels, container images, and gate commands", () => {
    expect(() => validateCiWorkflow(validWorkflow)).not.toThrow();
  });

  it("rejects jobs missing the docker runner label", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
          "    runs-on: [self-hosted, linux]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
        ),
      ),
    ).toThrow(/job client runs-on is missing required label docker/);
  });

  it("rejects jobs missing a container image", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
          "    runs-on: [self-hosted, linux, docker]\n    steps:\n      - name: Install",
        ),
      ),
    ).toThrow(/job client must declare a container image/);
  });

  it("rejects jobs that do not depend on the policy job", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "  client:\n    name: Client\n    needs: ci-policy\n",
          "  client:\n    name: Client\n",
        ),
      ),
    ).toThrow(/job client must depend on ci-policy/);
  });

  it("rejects null containers", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
          "    runs-on: [self-hosted, linux, docker]\n    container: null\n    steps:\n      - name: Install",
        ),
      ),
    ).toThrow(/job client must declare a container image/);
  });

  it.each(["null", "false"])(
    "rejects inline container image value %s",
    (imageValue) => {
      expect(() =>
        validateCiWorkflow(
          validWorkflow.replace(
            "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
            `    runs-on: [self-hosted, linux, docker]\n    container: { image: ${imageValue} }\n    steps:\n      - name: Install`,
          ),
        ),
      ).toThrow(/job client must declare a container image/);
    },
  );

  it("accepts inline container image after options", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
          "    runs-on: [self-hosted, linux, docker]\n    container: { options: --cpus 1, image: node:22-bookworm }\n    steps:\n      - name: Install",
        ),
      ),
    ).not.toThrow();
  });

  it("rejects inline empty mapping container image values", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
          "    runs-on: [self-hosted, linux, docker]\n    container: { image: {} }\n    steps:\n      - name: Install",
        ),
      ),
    ).toThrow(/job client must declare a container image/);
  });

  it.each(["null", "false"])(
    "rejects block container image value %s",
    (imageValue) => {
      expect(() =>
        validateCiWorkflow(
          validWorkflow.replace(
            "    runs-on: [self-hosted, linux, docker]\n    container: node:22-bookworm\n    steps:\n      - name: Install",
            `    runs-on: [self-hosted, linux, docker]\n    container:\n      image: ${imageValue}\n    steps:\n      - name: Install`,
          ),
        ),
      ).toThrow(/job client must declare a container image/);
    },
  );

  it("rejects jobs missing a required gate command", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace("        run: npm run beta-security-review:check\n", ""),
      ),
    ).toThrow(
      /job client is missing required run command: npm run beta-security-review:check/,
    );
  });

  it("rejects client jobs missing the mobile contract test gate", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "      - name: Test mobile contract\n        run: npm run mobile-contract:test\n",
          "",
        ),
      ),
    ).toThrow(
      /job client is missing required run command: npm run mobile-contract:test/,
    );
  });

  it("rejects rust advisory jobs missing lockfile verification", () => {
    expect(() =>
      validateCiWorkflow(
        validWorkflow.replace(
          "        run: cargo metadata --locked --format-version 1 > /dev/null\n",
          "",
        ),
      ),
    ).toThrow(
      /job rust-advisories is missing required run command: cargo metadata --locked --format-version 1 > \/dev\/null/,
    );
  });
});
