# Spec Delta

## Purpose

Maps an mTLS peer (certificate URI SAN plus source IP) to the run it is allowed to act for, so a stolen key is useless outside its pod (`design/agent-run.md` §4.2).

## ADDED Requirements

### Requirement: Identity resolution
Given a URI SAN `spiffe://dark-factory/ns/<ns>/pod/<name>/<uid>` and a peer IP, the provider SHALL return the run id only if: the namespace is the sandbox namespace; the pod exists in the provider's cache with that UID; the pod is owned by a Sandbox carrying this instance's label and a run id label; and the pod IP equals the peer IP. Otherwise it SHALL return nothing.

#### Scenario: Valid peer
- **WHEN** pod `run-df-1-1` with UID U and IP 10.0.3.7 connects from 10.0.3.7 with SAN for U
- **THEN** the run id of run 1 of `DF-1` is returned

#### Scenario: UID mismatch after pod recreation
- **WHEN** the SAN carries an old UID of a recreated pod with the same name
- **THEN** nothing is returned

#### Scenario: Pod not created by this instance
- **WHEN** the pod belongs to a Sandbox without this instance's label
- **THEN** nothing is returned

#### Scenario: Malformed SAN
- **WHEN** the SAN isn't a `spiffe://dark-factory/...` URI
- **THEN** nothing is returned
