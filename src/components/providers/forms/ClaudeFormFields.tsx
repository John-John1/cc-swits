import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ScrollArea } from "@/components/ui/scroll-area";
import { toast } from "sonner";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { CodexAutoAuthSection } from "./CodexAutoAuthSection";
import { GeminiAutoAuthSection } from "./GeminiAutoAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
} from "@/lib/api/copilot";
import type { CopilotModel } from "@/lib/api/copilot";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ManagedAuthProvider } from "@/lib/api/auth";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
  ClaudeAppExactModelMapping,
} from "@/types";
import type { TemplateValueConfig } from "@/config/claudeProviderPresets";

interface EndpointCandidate {
  url: string;
}

interface ClaudeFormFieldsProps {
  providerId?: string;
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  isCopilotPreset?: boolean;
  usesOAuth?: boolean;
  managedAuthProvider?: ManagedAuthProvider | null;
  isManagedAuthAuthenticated?: boolean;
  selectedManagedAccountId?: string | null;
  onManagedAccountSelect?: (accountId: string | null) => void;
  templateValueEntries: Array<[string, TemplateValueConfig]>;
  templateValues: Record<string, TemplateValueConfig>;
  templatePresetName: string;
  onTemplateValueChange: (key: string, value: string) => void;
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;
  shouldShowModelSelector: boolean;
  claudeModel: string;
  reasoningModel: string;
  defaultHaikuModel: string;
  defaultSonnetModel: string;
  defaultOpusModel: string;
  onModelChange: (
    field:
      | "ANTHROPIC_MODEL"
      | "ANTHROPIC_REASONING_MODEL"
      | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
      | "ANTHROPIC_DEFAULT_SONNET_MODEL"
      | "ANTHROPIC_DEFAULT_OPUS_MODEL",
    value: string,
  ) => void;
  speedTestEndpoints: EndpointCandidate[];
  apiFormat: ClaudeApiFormat;
  onApiFormatChange: (format: ClaudeApiFormat) => void;
  apiKeyField: ClaudeApiKeyField;
  onApiKeyFieldChange: (field: ClaudeApiKeyField) => void;
  isFullUrl: boolean;
  onFullUrlChange: (value: boolean) => void;
  claudeAppExactModelMappings: ClaudeAppExactModelMapping[];
  onClaudeAppExactModelMappingChange: (
    index: number,
    field: keyof ClaudeAppExactModelMapping,
    value: string,
  ) => void;
  onAddClaudeAppExactModelMapping: () => void;
  onRemoveClaudeAppExactModelMapping: (index: number) => void;
  claudeAppObservedSourceModels: string[];
  claudeAppFetchedTargetModels: string[];
  isFetchingClaudeAppTargetModels: boolean;
  isRefreshingClaudeAppObservedModels: boolean;
  onFetchClaudeAppTargetModels: () => void | Promise<void>;
  onRefreshClaudeAppObservedModels: () => void | Promise<void>;
  onClearClaudeAppObservedModels: () => void | Promise<void>;
  onClearClaudeAppFetchedTargetModels: () => void | Promise<void>;
}

export function ClaudeFormFields({
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  isCopilotPreset,
  usesOAuth,
  managedAuthProvider,
  isManagedAuthAuthenticated,
  selectedManagedAccountId,
  onManagedAccountSelect,
  templateValueEntries,
  templateValues,
  templatePresetName,
  onTemplateValueChange,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelSelector,
  claudeModel,
  reasoningModel,
  defaultHaikuModel,
  defaultSonnetModel,
  defaultOpusModel,
  onModelChange,
  speedTestEndpoints,
  apiFormat,
  onApiFormatChange,
  apiKeyField,
  onApiKeyFieldChange,
  isFullUrl,
  onFullUrlChange,
  claudeAppExactModelMappings,
  onClaudeAppExactModelMappingChange,
  onAddClaudeAppExactModelMapping,
  onRemoveClaudeAppExactModelMapping,
  claudeAppObservedSourceModels,
  claudeAppFetchedTargetModels,
  isFetchingClaudeAppTargetModels,
  isRefreshingClaudeAppObservedModels,
  onFetchClaudeAppTargetModels,
  onRefreshClaudeAppObservedModels,
  onClearClaudeAppObservedModels,
  onClearClaudeAppFetchedTargetModels,
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();
  const hasAnyAdvancedValue = !!(
    claudeModel ||
    reasoningModel ||
    defaultHaikuModel ||
    defaultSonnetModel ||
    defaultOpusModel ||
    apiFormat !== "anthropic" ||
    apiKeyField !== "ANTHROPIC_AUTH_TOKEN"
  );
  const [advancedExpanded, setAdvancedExpanded] = useState(hasAnyAdvancedValue);

  useEffect(() => {
    if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue]);

  const [copilotModels, setCopilotModels] = useState<CopilotModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [isClaudeAppTargetModelsOpen, setIsClaudeAppTargetModelsOpen] =
    useState(false);
  const [isClaudeAppObservedModelsOpen, setIsClaudeAppObservedModelsOpen] =
    useState(false);
  const [claudeAppTargetModelSearch, setClaudeAppTargetModelSearch] =
    useState("");
  const [claudeAppObservedModelSearch, setClaudeAppObservedModelSearch] =
    useState("");
  const supportsGenericModelFetch =
    !isCopilotPreset && managedAuthProvider !== "codex_auto";
  const looksLikeClaudeSourceModel = (value: string) => {
    const normalized = value.trim().toLowerCase();
    if (!normalized) return false;
    return (
      normalized.startsWith("claude-") ||
      /^haiku\b/.test(normalized) ||
      /^sonnet\b/.test(normalized) ||
      /^opus\b/.test(normalized)
    );
  };
  const codexAutoModelWarning =
    managedAuthProvider === "codex_auto" &&
    [
      claudeModel,
      reasoningModel,
      defaultHaikuModel,
      defaultSonnetModel,
      defaultOpusModel,
      ...claudeAppExactModelMappings.map((entry) => entry.targetModel),
    ].some((value) => looksLikeClaudeSourceModel(value))
      ? t("providerForm.claudeToCodexMappingWarning", {
          defaultValue:
            "这里填的是要转发到 Codex 的真实目标模型名，不要填 Opus / Sonnet / Haiku 这种 Claude 模型名。比如填 gpt-5.4、gpt-5.4-mini、o3、o4-mini。",
        })
      : null;
  const filteredClaudeAppTargetModels = claudeAppFetchedTargetModels.filter(
    (model) =>
      !claudeAppTargetModelSearch.trim() ||
      model
        .toLowerCase()
        .includes(claudeAppTargetModelSearch.trim().toLowerCase()),
  );
  const filteredClaudeAppObservedModels = claudeAppObservedSourceModels.filter(
    (model) =>
      !claudeAppObservedModelSearch.trim() ||
      model
        .toLowerCase()
        .includes(claudeAppObservedModelSearch.trim().toLowerCase()),
  );

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }

    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey, isFullUrl)
      .then((models) => {
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, isFullUrl, t]);

  useEffect(() => {
    if (!isCopilotPreset || !isManagedAuthAuthenticated) {
      setCopilotModels([]);
      setModelsLoading(false);
      return;
    }

    let cancelled = false;
    setModelsLoading(true);
    const fetchModels = selectedManagedAccountId
      ? copilotGetModelsForAccount(selectedManagedAccountId)
      : copilotGetModels();

    fetchModels
      .then((models) => {
        if (!cancelled) {
          setCopilotModels(models);
        }
      })
      .catch((err) => {
        console.warn("[Copilot] Failed to fetch models:", err);
        if (!cancelled) {
          toast.error(
            t("copilot.loadModelsFailed", {
              defaultValue: "加载 Copilot 模型列表失败",
            }),
          );
        }
      })
      .finally(() => {
        if (!cancelled) {
          setModelsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [isCopilotPreset, isManagedAuthAuthenticated, selectedManagedAccountId, t]);

  useEffect(() => {
    if (isClaudeAppObservedModelsOpen && providerId) {
      void onRefreshClaudeAppObservedModels();
    }
  }, [
    isClaudeAppObservedModelsOpen,
    onRefreshClaudeAppObservedModels,
    providerId,
  ]);

  const renderManagedAuthSection = () => {
    if (managedAuthProvider === "github_copilot") {
      return (
        <CopilotAuthSection
          selectedAccountId={selectedManagedAccountId}
          onAccountSelect={onManagedAccountSelect}
        />
      );
    }

    if (managedAuthProvider === "codex_auto") {
      return (
        <CodexAutoAuthSection
          selectedAccountId={selectedManagedAccountId}
          onAccountSelect={onManagedAccountSelect}
        />
      );
    }

    if (managedAuthProvider === "gemini_auto") {
      return (
        <GeminiAutoAuthSection
          selectedAccountId={selectedManagedAccountId}
          onAccountSelect={onManagedAccountSelect}
        />
      );
    }

    return null;
  };

  const renderModelInput = (
    id: string,
    value: string,
    field: ClaudeFormFieldsProps["onModelChange"] extends (
      f: infer F,
      v: string,
    ) => void
      ? F
      : never,
    placeholder?: string,
  ) => {
    if (isCopilotPreset && copilotModels.length > 0) {
      const grouped: Record<string, CopilotModel[]> = {};
      for (const model of copilotModels) {
        const vendor = model.vendor || "Other";
        if (!grouped[vendor]) {
          grouped[vendor] = [];
        }
        grouped[vendor].push(model);
      }
      const vendors = Object.keys(grouped).sort();

      return (
        <div className="flex gap-1">
          <Input
            id={id}
            type="text"
            value={value}
            onChange={(e) => onModelChange(field, e.target.value)}
            placeholder={placeholder}
            autoComplete="off"
            className="flex-1"
          />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon" className="shrink-0">
                <ChevronDown className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="end"
              className="z-[200] max-h-64 overflow-y-auto"
            >
              {vendors.map((vendor, index) => (
                <div key={vendor}>
                  {index > 0 && <DropdownMenuSeparator />}
                  <DropdownMenuLabel>{vendor}</DropdownMenuLabel>
                  {grouped[vendor].map((model) => (
                    <DropdownMenuItem
                      key={model.id}
                      onSelect={() => onModelChange(field, model.id)}
                    >
                      {model.id}
                    </DropdownMenuItem>
                  ))}
                </div>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      );
    }

    if (isCopilotPreset && modelsLoading) {
      return (
        <div className="flex gap-1">
          <Input
            id={id}
            type="text"
            value={value}
            onChange={(e) => onModelChange(field, e.target.value)}
            placeholder={placeholder}
            autoComplete="off"
            className="flex-1"
          />
          <Button variant="outline" size="icon" className="shrink-0" disabled>
            <Loader2 className="h-4 w-4 animate-spin" />
          </Button>
        </div>
      );
    }

    return (
      <ModelInputWithFetch
        id={id}
        value={value}
        onChange={(v) => onModelChange(field, v)}
        placeholder={placeholder}
        fetchedModels={fetchedModels}
        isLoading={isFetchingModels}
      />
    );
  };

  const renderModelDiscoveryPanel = (
    title: string,
    description: string,
    isOpen: boolean,
    onOpenChange: (open: boolean) => void,
    searchValue: string,
    onSearchChange: (value: string) => void,
    models: string[],
    emptyText: string,
    trigger: import("react").ReactNode,
    onClear: () => void | Promise<void>,
  ) => (
    <Collapsible open={isOpen} onOpenChange={onOpenChange}>
      <div className="rounded-xl border border-border/60 bg-muted/20">
        <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
          <div className="space-y-1">
            <p className="text-sm font-medium">{title}</p>
            <p className="text-xs text-muted-foreground">{description}</p>
          </div>
          <div className="flex items-center gap-2">
            {trigger}
            <CollapsibleTrigger asChild>
              <Button type="button" variant="outline" size="sm" className="h-8 gap-1.5">
                {isOpen ? (
                  <ChevronDown className="h-3.5 w-3.5" />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5" />
                )}
                {models.length}
              </Button>
            </CollapsibleTrigger>
          </div>
        </div>
        <CollapsibleContent className="border-t border-border/60 px-4 py-3">
          <div className="space-y-3">
            <div className="flex flex-wrap items-center gap-2">
              <Input
                value={searchValue}
                onChange={(event) => onSearchChange(event.target.value)}
                placeholder="搜索模型名"
                className="h-9 max-w-xs"
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => void onClear()}
                className="h-8 px-3 text-muted-foreground"
              >
                <Trash2 className="mr-1 h-3.5 w-3.5" />
                清空
              </Button>
            </div>
            {models.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border/70 px-3 py-4 text-sm text-muted-foreground">
                {emptyText}
              </div>
            ) : (
              <ScrollArea className="max-h-40 rounded-lg border border-border/70 px-3 py-3">
                <div className="flex flex-wrap gap-2">
                  {models.map((model) => (
                    <Badge
                      key={model}
                      variant="secondary"
                      className="rounded-md px-2.5 py-1 text-xs font-normal"
                    >
                      {model}
                    </Badge>
                  ))}
                </div>
              </ScrollArea>
            )}
          </div>
        </CollapsibleContent>
      </div>
    </Collapsible>
  );

  return (
    <>
      {renderManagedAuthSection()}

      {shouldShowApiKey && !usesOAuth && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}

      {templateValueEntries.length > 0 && (
        <div className="space-y-3">
          <FormLabel>
            {t("providerForm.parameterConfig", {
              name: templatePresetName,
              defaultValue: `${templatePresetName} 参数配置`,
            })}
          </FormLabel>
          <div className="space-y-4">
            {templateValueEntries.map(([key, config]) => (
              <div key={key} className="space-y-2">
                <FormLabel htmlFor={`template-${key}`}>{config.label}</FormLabel>
                <Input
                  id={`template-${key}`}
                  type="text"
                  required
                  value={
                    templateValues[key]?.editorValue ??
                    config.editorValue ??
                    config.defaultValue ??
                    ""
                  }
                  onChange={(e) => onTemplateValueChange(key, e.target.value)}
                  placeholder={config.placeholder || config.label}
                  autoComplete="off"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder")}
          hint={
            apiFormat === "openai_responses"
              ? t("providerForm.apiHintResponses")
              : apiFormat === "openai_chat"
                ? t("providerForm.apiHintOAI")
                : t("providerForm.apiHint")
          }
          onManageClick={() => onEndpointModalToggle(true)}
          showFullUrlToggle={true}
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
        />
      )}

      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="claude"
          providerId={providerId}
          value={baseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}

      {shouldShowModelSelector && (
        <Collapsible open={advancedExpanded} onOpenChange={setAdvancedExpanded}>
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
            >
              {advancedExpanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("providerForm.advancedOptionsToggle")}
            </Button>
          </CollapsibleTrigger>
          {!advancedExpanded && (
            <p className="mt-1 ml-1 text-xs text-muted-foreground">
              {t("providerForm.advancedOptionsHint")}
            </p>
          )}
          <CollapsibleContent className="space-y-4 pt-2">
            {category !== "cloud_provider" && (
              <div className="space-y-2">
                <FormLabel htmlFor="apiFormat">
                  {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
                </FormLabel>
                <Select value={apiFormat} onValueChange={onApiFormatChange}>
                  <SelectTrigger id="apiFormat" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="anthropic">
                      {t("providerForm.apiFormatAnthropic", {
                        defaultValue: "Anthropic Messages (原生)",
                      })}
                    </SelectItem>
                    <SelectItem value="openai_chat">
                      {t("providerForm.apiFormatOpenAIChat", {
                        defaultValue: "OpenAI Chat Completions (需转换)",
                      })}
                    </SelectItem>
                    <SelectItem value="openai_responses">
                      {t("providerForm.apiFormatOpenAIResponses", {
                        defaultValue: "OpenAI Responses API (需转换)",
                      })}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  {t("providerForm.apiFormatHint", {
                    defaultValue: "选择供应商 API 的输入格式",
                  })}
                </p>
              </div>
            )}

            <div className="space-y-2">
              <FormLabel>
                {t("providerForm.authField", { defaultValue: "认证字段" })}
              </FormLabel>
              <Select
                value={apiKeyField}
                onValueChange={(v) => onApiKeyFieldChange(v as ClaudeApiKeyField)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                    {t("providerForm.authFieldAuthToken", {
                      defaultValue: "ANTHROPIC_AUTH_TOKEN（默认）",
                    })}
                  </SelectItem>
                  <SelectItem value="ANTHROPIC_API_KEY">
                    {t("providerForm.authFieldApiKey", {
                      defaultValue: "ANTHROPIC_API_KEY",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.authFieldHint", {
                  defaultValue: "选择写入配置的认证环境变量名",
                })}
              </p>
            </div>

            <div className="space-y-1 border-t pt-2">
              <div className="flex items-center justify-between">
                <FormLabel>{t("providerForm.modelMappingLabel")}</FormLabel>
                {supportsGenericModelFetch && (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleFetchModels}
                    disabled={isFetchingModels}
                    className="h-7 gap-1"
                  >
                    {isFetchingModels ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Download className="h-3.5 w-3.5" />
                    )}
                    {t("providerForm.fetchModels")}
                  </Button>
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                {managedAuthProvider === "codex_auto"
                  ? t("providerForm.codexAutoModelMappingHint", {
                      defaultValue:
                        "这里配置的是 Claude 请求族到 Codex 真实模型名的映射。比如 Sonnet -> gpt-5.4，Opus -> o3，Haiku -> gpt-5.4-mini。",
                    })
                  : t("providerForm.modelMappingHint")}
              </p>
              {codexAutoModelWarning && (
                <p className="text-xs text-red-500">{codexAutoModelWarning}</p>
              )}
            </div>
            <div className="grid gap-3 xl:grid-cols-2">
              {renderModelDiscoveryPanel(
                "已获取目标模型",
                "手动点击按钮后，通过当前 provider 的轻量探测或模型列表接口持久保存。",
                isClaudeAppTargetModelsOpen,
                setIsClaudeAppTargetModelsOpen,
                claudeAppTargetModelSearch,
                setClaudeAppTargetModelSearch,
                filteredClaudeAppTargetModels,
                providerId
                  ? "还没有已获取的目标模型，点击右侧按钮拉取。"
                  : "请先保存 provider，再获取目标模型。",
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setIsClaudeAppTargetModelsOpen(true);
                    void onFetchClaudeAppTargetModels();
                  }}
                  disabled={!providerId || isFetchingClaudeAppTargetModels}
                  className="h-8 gap-1.5"
                >
                  {isFetchingClaudeAppTargetModels ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Download className="h-3.5 w-3.5" />
                  )}
                  获取目标模型
                </Button>,
                onClearClaudeAppFetchedTargetModels,
              )}
              {renderModelDiscoveryPanel(
                "已发现官方 App 源模型",
                "运行时从 wrapper 参数和 /claude-app 原始请求里自动去重记录，并持久保留。",
                isClaudeAppObservedModelsOpen,
                setIsClaudeAppObservedModelsOpen,
                claudeAppObservedModelSearch,
                setClaudeAppObservedModelSearch,
                filteredClaudeAppObservedModels,
                providerId
                  ? "还没有抓到官方 App 的实际模型名，等你运行后会自动出现在这里。"
                  : "请先保存 provider，再查看官方 App 运行时抓到的模型名。",
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setIsClaudeAppObservedModelsOpen(true);
                    void onRefreshClaudeAppObservedModels();
                  }}
                  disabled={!providerId || isRefreshingClaudeAppObservedModels}
                  className="h-8 gap-1.5"
                >
                  {isRefreshingClaudeAppObservedModels ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <RefreshCw className="h-3.5 w-3.5" />
                  )}
                  同步源模型
                </Button>,
                onClearClaudeAppObservedModels,
              )}
            </div>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <div className="space-y-2">
                <FormLabel htmlFor="claudeModel">
                  {t("providerForm.anthropicModel", {
                    defaultValue: "主模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeModel",
                  claudeModel,
                  "ANTHROPIC_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                )}
              </div>

              <div className="space-y-2">
                <FormLabel htmlFor="reasoningModel">
                  {t("providerForm.anthropicReasoningModel")}
                </FormLabel>
                {renderModelInput(
                  "reasoningModel",
                  reasoningModel,
                  "ANTHROPIC_REASONING_MODEL",
                )}
              </div>

              <div className="space-y-2">
                <FormLabel htmlFor="claudeDefaultHaikuModel">
                  {t("providerForm.anthropicDefaultHaikuModel", {
                    defaultValue: "Haiku 默认模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeDefaultHaikuModel",
                  defaultHaikuModel,
                  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                  t("providerForm.haikuModelPlaceholder", {
                    defaultValue: "",
                  }),
                )}
              </div>

              <div className="space-y-2">
                <FormLabel htmlFor="claudeDefaultSonnetModel">
                  {t("providerForm.anthropicDefaultSonnetModel", {
                    defaultValue: "Sonnet 默认模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeDefaultSonnetModel",
                  defaultSonnetModel,
                  "ANTHROPIC_DEFAULT_SONNET_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                )}
              </div>

              <div className="space-y-2">
                <FormLabel htmlFor="claudeDefaultOpusModel">
                  {t("providerForm.anthropicDefaultOpusModel", {
                    defaultValue: "Opus 默认模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeDefaultOpusModel",
                  defaultOpusModel,
                  "ANTHROPIC_DEFAULT_OPUS_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                )}
              </div>
            </div>
            <div className="space-y-3 rounded-xl border border-border/60 bg-muted/20 p-4">
              <div className="flex items-center justify-between gap-3">
                <div className="space-y-1">
                  <FormLabel className="text-sm font-medium">
                    Claude App 精确模型映射
                  </FormLabel>
                  <p className="text-xs text-muted-foreground">
                    左边填官方 App 运行时真实出现的内部模型名，右边填要转发到上游的真实目标模型名。
                  </p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={onAddClaudeAppExactModelMapping}
                  className="h-8 gap-1.5"
                >
                  <Plus className="h-3.5 w-3.5" />
                  添加精确映射
                </Button>
              </div>
              {claudeAppExactModelMappings.length === 0 ? (
                <div className="rounded-lg border border-dashed border-border/70 px-3 py-4 text-sm text-muted-foreground">
                  还没有精确映射。家族映射已经能覆盖大多数情况；只有遇到特殊内部模型名时再单独补这里。
                </div>
              ) : (
                <div className="space-y-3">
                  {claudeAppExactModelMappings.map((entry, index) => (
                    <div
                      key={`${index}-${entry.sourceModel}-${entry.targetModel}`}
                      className="grid gap-3 rounded-lg border border-border/60 bg-background px-3 py-3 md:grid-cols-[1fr_1fr_auto]"
                    >
                      <Input
                        value={entry.sourceModel}
                        onChange={(event) =>
                          onClaudeAppExactModelMappingChange(
                            index,
                            "sourceModel",
                            event.target.value,
                          )
                        }
                        placeholder="claude-sonnet-5-0"
                      />
                      <Input
                        value={entry.targetModel}
                        onChange={(event) =>
                          onClaudeAppExactModelMappingChange(
                            index,
                            "targetModel",
                            event.target.value,
                          )
                        }
                        placeholder="gpt-5.4"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => onRemoveClaudeAppExactModelMapping(index)}
                        className="h-10 px-3 text-muted-foreground hover:text-destructive"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}
    </>
  );
}
