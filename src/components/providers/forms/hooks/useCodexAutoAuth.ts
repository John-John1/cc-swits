import { useManagedAuth } from "./useManagedAuth";

export function useCodexAutoAuth() {
  const managedAuth = useManagedAuth("codex_auto");
  const defaultAccount =
    managedAuth.accounts.find(
      (account) => account.id === managedAuth.defaultAccountId,
    ) ?? managedAuth.accounts[0];

  return {
    ...managedAuth,
    username: defaultAccount?.login ?? null,
  };
}
