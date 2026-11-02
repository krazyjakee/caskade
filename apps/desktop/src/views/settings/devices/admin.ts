import { useActiveAccount } from "../../../api/accounts";

/** Whether the signed-in account may change devices: an admin (the browser
 * preview, without accounts, may too). The core refuses anyone else. */
export function useDeviceAdmin(): boolean {
  const account = useActiveAccount();
  return account === undefined || account?.role === "admin";
}
