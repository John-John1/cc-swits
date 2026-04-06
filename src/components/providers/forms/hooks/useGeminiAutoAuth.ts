import { useManagedAuth } from "./useManagedAuth";

export function useGeminiAutoAuth() {
  const managedAuth = useManagedAuth("gemini_auto");
  const defaultAccount =
    managedAuth.accounts.find(
      (account) => account.id === managedAuth.defaultAccountId,
    ) ?? managedAuth.accounts[0];

  return {
    ...managedAuth,
    username: defaultAccount?.login ?? null,
  };
}
