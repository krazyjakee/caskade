/** Console mode's top bar (#148): search, activity and the account. No
 * window controls: the window is fullscreen. */

import { ActivityLink } from "../ActivityLink";
import { AccountMenu } from "../accounts/AccountMenu";
import { SearchBox } from "../SearchBox";

export function ConsoleTopbar() {
  return (
    <header
      data-cs="console-topbar"
      className="relative z-10 flex h-topbar shrink-0 items-center gap-6 px-8"
    >
      <SearchBox />
      <div data-part="actions" className="ml-auto flex items-center gap-5">
        <ActivityLink className="size-6" />
        <AccountMenu size="md" />
      </div>
    </header>
  );
}
