## Purpose

Build a project's sandbox image from the `.dark-factory/` directory of its repository on a human's request, push it to the operator's registry, keep the build's logs, and decide from the project's builds whether its runs may start and with which image.

## ADDED Requirements

### Requirement: Feature enablement
Image builds SHALL be offered only when the instance configuration section `imageBuild` sets namespace, registry prefix, push Secret name, builder image and pusher image. Otherwise the project form SHALL NOT offer "Build from repository" and SHALL name the missing keys to board members. The push Secret's existence SHALL NOT be checked.

#### Scenario: Incomplete configuration
- **WHEN** `imageBuild` sets everything but the push Secret name
- **THEN** the project form offers no "Build from repository" and names `imageBuild.pushSecret` as missing

#### Scenario: Missing push Secret
- **WHEN** the configured push Secret doesn't exist and a build's pod is created
- **THEN** no container of the pod starts and the build fails with a reason naming the missing Secret, before its deadline

### Requirement: Startup verification of the build namespace
When image builds are enabled, the control plane SHALL verify at startup that the build namespace exists and that it may create, get, list, watch and delete pods, get pod logs, create and delete ConfigMaps and list and watch events there, and SHALL refuse to start with a message naming each missing item. It SHALL never create the namespace.

#### Scenario: Missing log permission
- **WHEN** image builds are enabled and the control plane may not get `pods/log` in the build namespace
- **THEN** it exits non-zero naming that permission

### Requirement: Starting a build
A board member who may edit a project SHALL be able to start a build of a project that builds its image, with "Build image". Switching a project to "Build from repository" SHALL start a build. Each build SHALL get the next build number of its project and SHALL record who started it and the commit of the base branch it reads.

#### Scenario: Switching to build
- **WHEN** a board member switches project `DF` to "Build from repository" and saves
- **THEN** build 1 of `DF` starts at the base branch's current commit

#### Scenario: Rebuild of an unchanged context
- **WHEN** a board member presses "Build image" and `.dark-factory/` is unchanged since build 3
- **THEN** build 4 starts and produces a new image tag with the same context hash

### Requirement: Build context
A build SHALL read every file under `.dark-factory/` at its commit. It SHALL fail before a pod is created when `.dark-factory/Containerfile` is missing, the directory holds more than 200 entries or more than 900 KiB of file content, it contains a symlink or submodule, or the forge doesn't support repository reads. The failure reason SHALL be shown on the build.

#### Scenario: Missing Containerfile
- **WHEN** a build starts and `.dark-factory/` has no `Containerfile`
- **THEN** the build fails with a reason naming `.dark-factory/Containerfile` and no pod is created

#### Scenario: Symlink in the context
- **WHEN** `.dark-factory/` contains a symlink
- **THEN** the build fails naming the symlink's path

#### Scenario: Context too large
- **WHEN** the files under `.dark-factory/` total 2 MiB
- **THEN** the build fails naming the 900 KiB limit

### Requirement: Image name
A build SHALL push the image `<registry prefix>/<project key in lower case>:<context hash>-<build number>`. The context hash SHALL be the hex SHA-256 over the lines `<path>\0<blob SHA>\n` of all context files, sorted by path.

#### Scenario: Changed file
- **WHEN** a dependency version in `.dark-factory/Containerfile` changes between build 2 and build 3
- **THEN** build 3's tag has a different context hash than build 2's

#### Scenario: Renamed file
- **WHEN** `.dark-factory/a.sh` is renamed to `.dark-factory/b.sh` without content change
- **THEN** the context hash changes

### Requirement: Builder without credentials
The builder SHALL receive only the build context and SHALL write the image as an image tarball in the format `docker save` writes. Only the pusher, which starts after the builder succeeded, SHALL receive the push Secret. The build SHALL succeed when the pusher reports the pushed digest.

#### Scenario: Builder environment
- **WHEN** a build pod is inspected
- **THEN** the builder container mounts no Secret and has no service account token, and the pusher container mounts the push Secret

#### Scenario: Builder fails
- **WHEN** a `RUN` step of the Containerfile fails
- **THEN** the pusher doesn't start and the build fails

### Requirement: Build deadline
A build SHALL fail when it runs longer than `imageBuild.deadlineSeconds` (default 1800).

#### Scenario: Slow build
- **WHEN** a build is still running after 30 minutes with default settings
- **THEN** its pod is stopped and the build fails with reason "deadline exceeded"

### Requirement: Build concurrency
Starting a build of a project SHALL cancel that project's running build. With `imageBuild.maxConcurrent` set, at most that many builds SHALL run at a time across projects; further builds SHALL wait as `queued` and start in order.

#### Scenario: Newer build replaces running build
- **WHEN** build 5 of `DF` is running and a board member starts build 6
- **THEN** build 5 is cancelled and build 6 starts

#### Scenario: Instance limit
- **WHEN** `maxConcurrent` is 2, two builds of other projects run and build 1 of `DF` is started
- **THEN** build 1 is `queued` until one of the running builds ends

### Requirement: Cancelling a build
A board member who may edit a project SHALL be able to cancel its queued or running build. Cancelling SHALL delete the build's pod and context, keep its logs and mark the build `cancelled`.

#### Scenario: Cancel a running build
- **WHEN** a board member cancels running build 4 of `DF`
- **THEN** its pod is deleted and the build is `cancelled` with its logs so far

#### Scenario: Pod created after the cancel
- **WHEN** build 4 of `DF` is cancelled while its pod is being created, and the replica creating it stops before checking the build again
- **THEN** the pod is deleted within one scheduler tick

### Requirement: Build logs
The control plane SHALL store the output of builder and pusher for each build, in order, while it runs, keeping at most the last 10 MiB. A build page SHALL show the build's state, number, commit, context hash, image and digest, who started it and its log, and SHALL append new log output live.

#### Scenario: Live log
- **WHEN** a board member opens the page of a running build
- **THEN** new builder output appears without a reload

#### Scenario: Long log
- **WHEN** a build writes 15 MiB of output
- **THEN** the stored log holds the last 10 MiB and says that earlier output was dropped

#### Scenario: Escaped output
- **WHEN** builder output contains HTML
- **THEN** the build page shows it as text

### Requirement: Project image state
For a project that builds its image, the project image SHALL be the image of its latest successful build, by digest, when its latest build that wasn't cancelled succeeded. While that build is queued or running, failed, or doesn't exist, the project SHALL have no usable image. Cancelled builds SHALL be ignored.

#### Scenario: Failed build after a success
- **WHEN** build 5 of `DF` succeeded and build 6 failed
- **THEN** `DF` has no usable image until a later build succeeds

#### Scenario: Cancelled build after a success
- **WHEN** build 5 of `DF` succeeded and build 6 was cancelled
- **THEN** `DF` uses build 5's image

### Requirement: Build list
The project settings page SHALL list the project's builds, newest first, with number, state, start time and duration, each linking to its build page, and SHALL offer "Build image" and "Cancel build" to board members who may edit the project.

#### Scenario: Build list
- **WHEN** a board member opens the settings of a project with three builds
- **THEN** the builds are listed newest first with their states

### Requirement: Build actions are audited
Starting and cancelling a build SHALL write an audit entry with the project, build number and board member.

#### Scenario: Audited start
- **WHEN** a board member starts build 2 of `DF`
- **THEN** an audit entry names the board member, `DF` and build 2
