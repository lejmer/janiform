# Instructions for agents when writing Project and Issue descriptions

This document outlines instructions for agents when writing new Project descriptions and their corresponding Issue descriptions.

## Projects and Issues

In this team, we organize all work into *Projects* and *Issues*. A Project is a full feature or full improvement of a feature (similar in concept to Epics). An Issue is a specific implementation task (similar in concept to Stories).

## Writing Projects

When writing a project:
1. Create a folder in `docs/planning/projects/` and name it based on the name of the Project (use snake_case). This is the root folder for the Project.
2. Create a subfolder in the root of the Project called `issues`.
3. In the root of the Project, create a file called `descriptions.md`. It should contain:
   - The name of the Project (formatted as header level 2, `##`).
   - The context and requirements.
     - Definition of Done (Project-level).
     - Agent implementation notes (if any).

## Writing Issues for a Project

When writing an issue:
1. Locate the Project in `docs/planning/projects/`.
2. Create a file in the `issues` folder of the project:
   - Name it in the following way:
     - `<Issue ID> - <Issue name>.md`.
     - Example: `13 - Master-Server-project-skeleton-and-service-boundaries.md`.
     - The exact way you name the issue ID is not important, but keep it in ascending order so that the order the Issues should be implemented in is preserved.
   - The file should contain:
     - The name of the Issue (formatted as header level 2, `##`).
     - A description of the Issue.
     - Acceptance Criteria.
     - Optional: Unit tests (and/or when necessary, integration tests).
     - Optional: Notes/Constraints.

## Template for Projects

An example template for a `description.md` file:

```md
## My Project Name

A project description goes here.

**Context / Requirements (must be implemented as described)**

- The various requirements and context
- Non-goals in this project:
  - Some non-goals
  - Some more non-goals
  - Stuff we want to make sure is out-of-scope at this time

**Definition of Done (Project-level)**

- The various checks that define when the Project has reached a *done* state.

**Agent Implementation Notes**

- Assumptions, protocols, constraints, and so on, that an implementing agent needs to be aware of.
```

## Template for Issues

An example template for an issue `.md` file:

`52 - My Issue Name.md`:
```md
## My Issue Name

A description of the Issue we are implementing goes here.

**Acceptance Criteria**

* List of stuff that has to be full-filled for the Issue to be considered fully implemented.

**Notes/Constraints**

* Information that is good to be aware of
* Like ideas for libraries to use
* Constraints that we need to be aware of
* And so on...
```
