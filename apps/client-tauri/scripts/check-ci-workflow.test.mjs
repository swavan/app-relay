import { describe, expect, it } from "vitest";
import { validateCiWorkflow } from "./check-ci-workflow.mjs";

const workflow = (jobOverrides = "") => `name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm
${jobOverrides}`;

describe("ci workflow checker", () => {
  it("accepts jobs with the required Docker runner labels and container images", () => {
    expect(() =>
      validateCiWorkflow(
        workflow(`  rust:
    name: Rust
    needs:
      - ci-policy
    runs-on:
      - self-hosted
      - linux
      - docker
    container:
      image: rust:1.86-bookworm
`),
      ),
    ).not.toThrow();
  });

  it("rejects jobs missing the docker runner label", () => {
    expect(() =>
      validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux]
    container: node:22-bookworm
`),
    ).toThrow(/job client runs-on is missing required label docker/);
  });

  it("rejects jobs missing a container image", () => {
    expect(() =>
      validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
`),
    ).toThrow(/job client must declare a container image/);
  });

  it("rejects jobs that do not depend on the policy job", () => {
    expect(() =>
      validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm
`),
    ).toThrow(/job client must depend on ci-policy/);
  });

  it("rejects null containers", () => {
    expect(() =>
      validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: null
`),
    ).toThrow(/job client must declare a container image/);
  });

  it.each(["null", "false"])(
    "rejects inline container image value %s",
    (imageValue) => {
      expect(() =>
        validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: { image: ${imageValue} }
`),
      ).toThrow(/job client must declare a container image/);
    },
  );

  it("accepts inline container image after options", () => {
    expect(() =>
      validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: { options: --cpus 1, image: node:22-bookworm }
`),
    ).not.toThrow();
  });

  it("rejects inline empty mapping container image values", () => {
    expect(() =>
      validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container: { image: {} }
`),
    ).toThrow(/job client must declare a container image/);
  });

  it.each(["null", "false"])(
    "rejects block container image value %s",
    (imageValue) => {
      expect(() =>
        validateCiWorkflow(`name: CI

on:
  pull_request:

jobs:
  ci-policy:
    name: CI Policy
    runs-on: [self-hosted, linux, docker]
    container: node:22-bookworm

  client:
    name: Client
    needs: ci-policy
    runs-on: [self-hosted, linux, docker]
    container:
      image: ${imageValue}
`),
      ).toThrow(/job client must declare a container image/);
    },
  );
});
