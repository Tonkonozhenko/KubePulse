# Spec Delta

## Purpose

Lets users start a CronJob's Job outside its schedule, for one CronJob or several, after a confirmation naming what will run.

## ADDED Requirements

### Requirement: Run now on CronJobs
The system SHALL offer Run now on CronJobs from the `batch` group, in the row actions column, the row's right-click menu, and the detail panel. Other kinds MUST NOT offer it.

#### Scenario: CronJob row
- **WHEN** the user views a CronJobs table
- **THEN** each row offers Run now, and the row's right-click menu offers it too

#### Scenario: Detail panel
- **WHEN** the user opens a CronJob
- **THEN** the detail panel's actions include Run now

#### Scenario: Other kind
- **WHEN** the user views Jobs, Deployments, or a custom resource kind
- **THEN** no Run now action is offered

### Requirement: Run now creates a Job from the template
Running SHALL create one Job from the CronJob's current job template, with the template's labels and annotations, in the CronJob's namespace. The Job SHALL be owned by the CronJob, SHALL be marked as started by hand, and SHALL get a unique name starting with the CronJob's name. The CronJob itself MUST NOT change.

#### Scenario: Run a CronJob
- **WHEN** the user confirms Run now on a CronJob named `backup`
- **THEN** a new Job whose name starts with `backup-` appears, owned by `backup`, and its pods start

#### Scenario: Suspended CronJob
- **WHEN** the user runs a CronJob that is suspended
- **THEN** the Job is created, and the CronJob stays suspended

#### Scenario: Run twice
- **WHEN** the user runs the same CronJob twice
- **THEN** two Jobs exist, with different names

### Requirement: Run now requires confirmation
The system SHALL ask for confirmation naming the cluster and every CronJob with its namespace, and nothing SHALL run until the user confirms. Pressing Enter MUST NOT confirm. The confirmation shows no diff.

#### Scenario: Run one CronJob
- **WHEN** the user chooses Run now on a CronJob
- **THEN** a confirmation shows the cluster, namespace, and CronJob name, and no Job is created until the user confirms

#### Scenario: Enter does not confirm
- **WHEN** the confirmation is open and the user presses Enter
- **THEN** no Job is created

#### Scenario: Protected cluster, one CronJob
- **WHEN** the user runs one CronJob on a protected cluster
- **THEN** the confirm button appears only after the user types the CronJob's name

#### Scenario: Protected cluster, several CronJobs
- **WHEN** the user runs several CronJobs on a protected cluster
- **THEN** the confirm button appears only after the user types the cluster's name

### Requirement: Running several CronJobs
When CronJob rows are checked, the table SHALL offer to run them next to deleting them. The confirmation SHALL list every checked CronJob.

#### Scenario: Run selected
- **WHEN** the user checks two CronJobs and chooses Run selected
- **THEN** a confirmation shows the cluster and those two CronJobs with their namespaces

### Requirement: Run now reports each result
The system SHALL report progress while creating Jobs, SHALL name every CronJob that failed with the cluster's error, and SHALL name each Job it created. A failure MUST NOT stop the remaining runs. A CronJob that no longer exists SHALL be reported as failed.

#### Scenario: Created Jobs are named
- **WHEN** every run succeeds
- **THEN** the confirmation closes, and a notice names each Job that was created

#### Scenario: Some CronJobs fail
- **WHEN** one of three runs is denied
- **THEN** the other two Jobs are created, and the confirmation stays open naming the denied CronJob and the error

#### Scenario: Retry
- **WHEN** the confirmation shows failures and the user confirms again
- **THEN** only the failed CronJobs are run again, and no second Job is created for the ones that succeeded

#### Scenario: Cancel during runs
- **WHEN** the user cancels while runs are in progress
- **THEN** no further Jobs are created, and the Jobs already created stay

#### Scenario: Denied
- **WHEN** the user's account may read a CronJob but not create Jobs in its namespace, and the user confirms
- **THEN** the confirmation shows the cluster's denial for that CronJob, and no Job is created
