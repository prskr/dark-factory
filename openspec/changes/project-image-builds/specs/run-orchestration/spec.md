## ADDED Requirements

### Requirement: Runs wait for the project image
A due task SHALL NOT start a run while its project has no usable image: a project using "Build from repository" whose builds give no usable image (`image-builds`, "Project image state"), or a misconfigured project (`configuration-management`, "Project without usable image source"). The task SHALL stay `queued` and show why; its run SHALL start within one scheduler tick once the image is usable. Started runs SHALL NOT be affected.

#### Scenario: Build failed
- **WHEN** task `DF-4` is queued, `DF` uses "Build from repository" and its latest build failed
- **THEN** no run starts and the task shows that it waits for a successful image build

#### Scenario: Failed build of a former build project
- **WHEN** `DF` switched from "Build from repository" to "Image" after its latest build failed, and task `DF-4` is queued
- **THEN** a run starts with the project's image

#### Scenario: Build succeeds
- **WHEN** task `DF-4` waits for the image and build 7 of `DF` succeeds
- **THEN** a run of `DF-4` starts within one scheduler tick

#### Scenario: Running run unaffected
- **WHEN** run 2 of `DF-5` is running and a new build of `DF` starts
- **THEN** run 2 continues

### Requirement: Image of a run
Provisioning a run SHALL resolve the project image once and use it for the whole run: the project's image reference, the default image, or the digest reference `<image>@sha256:<digest>` of the project's latest successful build.

#### Scenario: Built image by digest
- **WHEN** a run of `DF` is provisioned and build 5 pushed `registry.example.org/df/df:ab12…-5` with digest `sha256:9f…`
- **THEN** the sandbox pod's image is `registry.example.org/df/df:ab12…-5@sha256:9f…`

#### Scenario: Image pinned for the run
- **WHEN** build 6 of `DF` succeeds while run 3 of `DF-1` is provisioning with build 5's image
- **THEN** run 3 keeps build 5's image
