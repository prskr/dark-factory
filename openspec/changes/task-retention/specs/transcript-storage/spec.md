# Spec Delta

## ADDED Requirements

### Requirement: Unreferenced blobs are deleted
When the transcript events of a deleted task are purged, the control plane SHALL delete every blob those events referred to that no remaining transcript event refers to. A blob stored concurrently by another run SHALL NOT be lost.

#### Scenario: Image only in the deleted task
- **WHEN** an image was posted only in `DF-1` and `DF-1` is purged
- **THEN** the blob store no longer holds that image

#### Scenario: Image shared with another task
- **WHEN** the same image appears in the transcripts of `DF-1` and `DF-2` and `DF-1` is purged
- **THEN** the blob store still returns the image for `DF-2`
