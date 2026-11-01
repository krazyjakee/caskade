# Installer activity

`ActivityProvider` consumes full, revisioned job snapshots from the generated
Tauri adapter. Older events and late list responses cannot overwrite newer jobs.
Completion invalidates library queries. Production never substitutes fixture
jobs for a missing backend; the fixture provider requires a development build
and `VITE_CASKADE_FIXTURES=1`.

The top bar opens file, folder and URL inputs. `InstallRootPicker` returns a
registered root ID; the backend checks eligibility again. Activity shows byte
and file progress, warnings, dependencies, verification and available strategies.
Answers carry the retained question revision and selected index. A job the
user started (Add game, a confirmed link or drop, an update, a retry) is
followed (`useFollowJob`): `JobQuestionHost`, in the shell, opens each question
it asks over whatever view is open, until it ends or the user picks Not now.
Activity's Review choices follows a waiting job the same way. Directory
adoption and reinstall decisions that the backend cannot answer are explanatory,
with cancellation available, rather than permission to move arbitrary files.

External links, file drops and paste outside editable fields enter the same
pending-request queue. Native file drops go to the drop zone under the pointer
(`drop/DropProvider.tsx`): the library asks the core to plan the drop first (a
shortcut's target, an executable's game folder) and the confirmation shows the
plan; path fields and the plugin list take the path themselves; a drop outside
every zone is a plain request. The confirmation dialog precedes root selection and job
creation. Browser file names are never treated as filesystem paths. Dismissal
does not fetch or execute the input.

React Aria manages menus, radio navigation, modal focus trapping/restoration,
Escape and keyboard activation. `data-spatial="focusable"` and focus-scope
attributes provide integration points for future console navigation. Physical
D-pad routing belongs to console mode (M5) and is not implemented here.

Tests cover progress, warnings, strategies, stale snapshots, waiting decisions,
errors, keyboard focus, shared root selection and confirmed ingress. The native
adapter tests exercise generated commands/events without a live installer.
