import React from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ExternalLink,
  Loader2,
  LogOut,
  Plus,
  User,
  X,
} from "lucide-react";
import { useCodexAutoAuth } from "./hooks/useCodexAutoAuth";
import type { ManagedAuthAccount } from "@/lib/api";

interface CodexAutoAuthSectionProps {
  className?: string;
  selectedAccountId?: string | null;
  onAccountSelect?: (accountId: string | null) => void;
}

export const CodexAutoAuthSection: React.FC<CodexAutoAuthSectionProps> = ({
  className,
  selectedAccountId,
  onAccountSelect,
}) => {
  const { t } = useTranslation();
  const {
    accounts,
    defaultAccountId,
    migrationError,
    hasAnyAccount,
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    isRemovingAccount,
    isSettingDefaultAccount,
    addAccount,
    removeAccount,
    setDefaultAccount,
    cancelAuth,
    logout,
  } = useCodexAutoAuth();

  const handleRemoveAccount = (accountId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    removeAccount(accountId);
    if (selectedAccountId === accountId) {
      onAccountSelect?.(null);
    }
  };

  const handleAccountSelect = (value: string) => {
    onAccountSelect?.(value === "none" ? null : value);
  };

  return (
    <div className={`space-y-4 ${className || ""}`}>
      <div className="flex items-center justify-between">
        <Label>
          {t("codexAuto.authStatus", {
            defaultValue: "Codex Auto 认证状态",
          })}
        </Label>
        <Badge
          variant={hasAnyAccount ? "default" : "secondary"}
          className={hasAnyAccount ? "bg-green-500 hover:bg-green-600" : ""}
        >
          {hasAnyAccount
            ? t("codexAuto.accountCount", {
                count: accounts.length,
                defaultValue: `${accounts.length} 个账号`,
              })
            : t("codexAuto.notAuthenticated", {
                defaultValue: "未认证",
              })}
        </Badge>
      </div>

      {migrationError && (
        <p className="text-sm text-amber-600 dark:text-amber-400">
          {t("codexAuto.migrationFailed", {
            error: migrationError,
            defaultValue: `认证数据同步失败：${migrationError}`,
          })}
        </p>
      )}

      {hasAnyAccount && onAccountSelect && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("codexAuto.selectAccount", {
              defaultValue: "选择账号",
            })}
          </Label>
          <Select
            value={selectedAccountId || "none"}
            onValueChange={handleAccountSelect}
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t("codexAuto.selectAccountPlaceholder", {
                  defaultValue: "选择一个 OpenAI 账号",
                })}
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">
                <span className="text-muted-foreground">
                  {t("codexAuto.useDefaultAccount", {
                    defaultValue: "使用默认账号",
                  })}
                </span>
              </SelectItem>
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  <div className="flex items-center gap-2">
                    <CodexAutoAccountAvatar account={account} />
                    <span>{account.login}</span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {hasAnyAccount && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("codexAuto.loggedInAccounts", {
              defaultValue: "已登录账号",
            })}
          </Label>
          <div className="space-y-1">
            {accounts.map((account) => (
              <div
                key={account.id}
                className="flex items-center justify-between rounded-md border bg-muted/30 p-2"
              >
                <div className="flex items-center gap-2">
                  <CodexAutoAccountAvatar account={account} />
                  <span className="text-sm font-medium">{account.login}</span>
                  {defaultAccountId === account.id && (
                    <Badge variant="secondary" className="text-xs">
                      {t("codexAuto.defaultAccount", {
                        defaultValue: "默认",
                      })}
                    </Badge>
                  )}
                  {selectedAccountId === account.id && (
                    <Badge variant="outline" className="text-xs">
                      {t("codexAuto.selected", {
                        defaultValue: "已选中",
                      })}
                    </Badge>
                  )}
                </div>
                <div className="flex items-center gap-1">
                  {defaultAccountId !== account.id && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-xs text-muted-foreground"
                      onClick={() => setDefaultAccount(account.id)}
                      disabled={isSettingDefaultAccount}
                    >
                      {t("codexAuto.setAsDefault", {
                        defaultValue: "设为默认",
                      })}
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-muted-foreground hover:text-red-500"
                    onClick={(e) => handleRemoveAccount(account.id, e)}
                    disabled={isRemovingAccount}
                    title={t("codexAuto.removeAccount", {
                      defaultValue: "移除账号",
                    })}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {!hasAnyAccount && pollingState === "idle" && (
        <Button
          type="button"
          onClick={addAccount}
          className="w-full"
          variant="outline"
        >
          {t("codexAuto.loginWithOpenAI", {
            defaultValue: "使用 OpenAI 登录",
          })}
        </Button>
      )}

      {hasAnyAccount && pollingState === "idle" && (
        <Button
          type="button"
          onClick={addAccount}
          className="w-full"
          variant="outline"
          disabled={isAddingAccount}
        >
          <Plus className="mr-2 h-4 w-4" />
          {t("codexAuto.addAnotherAccount", {
            defaultValue: "添加其他账号",
          })}
        </Button>
      )}

      {isPolling && deviceCode && (
        <div className="space-y-3 rounded-lg border border-border bg-muted/50 p-4">
          <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("codexAuto.waitingForAuth", {
              defaultValue: "正在等待 OpenAI 完成授权...",
            })}
          </div>

          <div className="space-y-2 text-center">
            <p className="text-xs text-muted-foreground">
              {t("codexAuto.browserHint", {
                defaultValue:
                  "浏览器会自动打开 OpenAI 登录页，完成后会自动同步到 CC Switch。",
              })}
            </p>
            <a
              href={deviceCode.verification_uri}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-sm text-blue-500 hover:underline"
            >
              {t("codexAuto.reopenAuthPage", {
                defaultValue: "重新打开授权页面",
              })}
              <ExternalLink className="h-3 w-3" />
            </a>
          </div>

          <div className="text-center">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={cancelAuth}
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
          </div>
        </div>
      )}

      {pollingState === "error" && error && (
        <div className="space-y-2">
          <p className="text-sm text-red-500">{error}</p>
          <div className="flex gap-2">
            <Button
              type="button"
              onClick={addAccount}
              variant="outline"
              size="sm"
            >
              {t("codexAuto.retry", { defaultValue: "重试" })}
            </Button>
            <Button
              type="button"
              onClick={cancelAuth}
              variant="ghost"
              size="sm"
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
          </div>
        </div>
      )}

      {hasAnyAccount && accounts.length > 1 && (
        <Button
          type="button"
          variant="outline"
          onClick={logout}
          className="w-full text-red-500 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-950"
        >
          <LogOut className="mr-2 h-4 w-4" />
          {t("codexAuto.logoutAll", {
            defaultValue: "退出全部账号",
          })}
        </Button>
      )}
    </div>
  );
};

const CodexAutoAccountAvatar: React.FC<{ account: ManagedAuthAccount }> = ({
  account,
}) => {
  const [failed, setFailed] = React.useState(false);

  if (!account.avatar_url || failed) {
    return <User className="h-5 w-5 text-muted-foreground" />;
  }

  return (
    <img
      src={account.avatar_url}
      alt={account.login}
      className="h-5 w-5 rounded-full"
      loading="lazy"
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  );
};

export default CodexAutoAuthSection;
