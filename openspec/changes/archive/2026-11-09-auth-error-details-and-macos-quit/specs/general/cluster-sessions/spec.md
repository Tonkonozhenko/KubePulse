## ADDED Requirements

### Requirement: Authentication failure details
When authentication fails, the failure reason SHALL name the specific cause instead of a generic message. For a credential helper that exits unsuccessfully, the reason SHALL name the helper command and its exit status, followed by the helper's error output exactly as written. The helper's standard output MUST NOT appear in the reason, and its error output MUST NOT be written to logs.

#### Scenario: Helper exits with an error
- **WHEN** a credential helper exits with a non-zero status after writing a message to its error output
- **THEN** the cluster's failure reason names the helper command and the exit status, and shows that message

#### Scenario: Helper standard output stays hidden
- **WHEN** a credential helper fails after writing a token to its standard output
- **THEN** the token appears neither in the failure reason nor in the logs

#### Scenario: Helper error output stays out of logs
- **WHEN** a credential helper fails after writing a message to its error output
- **THEN** that message appears in the failure reason and not in the logs

#### Scenario: Helper program missing
- **WHEN** the helper command named in the kubeconfig cannot be started
- **THEN** the failure reason names the command and says it could not be started

#### Scenario: Unusable helper response
- **WHEN** a credential helper exits successfully but its output is not a valid credential
- **THEN** the failure reason says the helper's response could not be read, without quoting that response

#### Scenario: Helper cannot prompt
- **WHEN** the system runs a credential helper
- **THEN** it tells the helper that it cannot prompt for input, and captures the helper's error output instead of passing it through

#### Scenario: Authentication fails after connecting
- **WHEN** a credential helper fails while refreshing credentials for a connected cluster
- **THEN** the degraded state's reason shows the same details as a failed connection attempt
