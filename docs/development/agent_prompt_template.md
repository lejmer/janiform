In this team, we mainly plan in Linear, so we organize all work into Projects and Issues. A Project is a full feature or full improvement of a feature (similar in concept to Epics). An Issue is a specific implementation task (similar in concept to Stories). Below, you will be given the full Project description, for context, followed by the specific Issue to implement.

---

# Project description



---

# Issue description



---

# IMPORTANT

Do not catch all exceptions unless absolutely necessary. If you do, ALWAYS log them, including a debug-level stack trace. Do NOT write overly defensive code. Always assume the user has enough control over the code base. No `getattr` just because "maybe the class isn't there in the users copy of the code". Always assume the user has the latest version of the code and that all the attributes the code expects is always there. Unnecessary use of `getattr` creates hard-to-find bugs and is bad practice.

Do not use global helpers for classes! Prefer static methods or class methods. If multiple classes need the same helper, do not copy the helper. Make an abstract base class they inherit instead. Always remember to use `@override` for overridden implementations (even for implementations of abstract methods and abstract properties).

If you find your working tree to be dirty, you have our explicit permission to run `git reset --hard HEAD` to clean it before you begin working. All prior work has already been merged, so there will be no data loss.

---

Begin by reading ./AGENTS.md and follow that exactly when writing code.

Now that the expectations are set, please do your best to plan your implementation of the Issue described above. If you find yourself unsure about something or if you want clarification, it is better to ask questions than to just impose your own assumptions.

NOTE: Remember to implement unit tests whenever you introduce new API to the codebase, and update tests when you make changes to function side effects, assumptions, or API!
