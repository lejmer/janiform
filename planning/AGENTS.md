# Instructions for agents when writing Project and Issue descriptions

This document outlines instructions for agents when writing new Issue descriptions, with Project descriptions and their corresponding Issue descriptions.

## Initiatives, Projects and Issues

In this team, we organize all work into *Initiatives*, *Projects* and *Issues*. An Initiative is a collection of related Projects. A Project is a full feature or full improvement of a feature (similar in concept to Epics). An Issue is a specific implementation task (similar in concept to Stories).

## Writing Initiatives

When writing an Initiative:
1. Create a folder in `planning/initiatives/` and name it based on the name of the Initiative (use snake_case), prefixed by a monotonically increasing ID. This is the root folder for the Initiative.
2. In the root of the Initiative, create a file called `description.md`. It should contain:
   - The name of the Initiative (formatted as header level 2, `##`).
   - The desired outcome.
   - The success signals.
   - Example projects that might be useful to add.

## Writing Projects for an Initiative

When writing a Project:
1. Create a folder in `planning/initiatives/<initiative_name>/` and name it based on the name of the Project (use snake_case), prefixed by a monotonically increasing ID. This is the root folder for the Project.
2. Create a subfolder in the root of the Project called `issues`.
3. In the root of the Project, create a file called `description.md`. It should contain:
   - The name of the Project (formatted as header level 2, `##`).
   - The context and requirements.
     - Definition of Done (Project-level).
     - Agent implementation notes (if any).
   - How it ties into the desired outcomes and/or success signals of the parent Initiative.

## Writing Issues for a Project

When writing an Issue:
1. Locate the Project in `planning/initiatives/`.
2. Create a file in the `issues` folder of the project:
   - Name it in the following way:
     - `<Issue ID> <Issue name>.md`.
     - Example: `ISSUE-13 My-Issue-Name.md`.
   - The file should contain:
     - The name of the Issue (formatted as header level 2, `##`).
     - A description of the Issue.
     - Acceptance Criteria.
     - Optional: Unit tests (and/or when necessary, integration tests).
     - Optional: Notes/Constraints.

## General writing rules

- Use `"` for citations and `'` for apostrophes. Avoid using `“`/`”` and `’`.
- Don't overuse `;`.
- Don't overuse `+` to delimit list items in text
- Don't overuse `+` in place of `and`.
- Don't write em-dashes (`—`).
