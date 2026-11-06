# Spec Delta

## MODIFIED Requirements

### Requirement: Language change applies after restart
A language change SHALL take effect on the next launch. The Settings tab SHALL say so and offer to restart, SHALL warn that restarting disconnects every cluster and closes every tab, and MUST NOT restart without the user choosing to.

#### Scenario: Change language
- **WHEN** the user changes the language
- **THEN** the interface stays in its current language, and the Settings tab offers a restart with that warning

#### Scenario: Restart now
- **WHEN** the user accepts the restart
- **THEN** the application restarts in the new language with no open tabs, and connects no cluster
