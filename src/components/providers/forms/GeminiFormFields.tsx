import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Info, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import type { ProviderCategory } from "@/types";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { GeminiAutoAuthSection } from "./GeminiAutoAuthSection";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";

interface EndpointCandidate {
  url: string;
}

interface GeminiFormFieldsProps {
  providerId?: string;
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;
  shouldShowModelField: boolean;
  model: string;
  onModelChange: (value: string) => void;
  speedTestEndpoints: EndpointCandidate[];
  managedAuthProvider?: "gemini_auto" | null;
  isManagedAuthAuthenticated?: boolean;
  selectedManagedAccountId?: string | null;
  onManagedAccountSelect?: (accountId: string | null) => void;
}

export function GeminiFormFields({
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelField,
  model,
  onModelChange,
  speedTestEndpoints,
  managedAuthProvider,
  isManagedAuthAuthenticated = false,
  selectedManagedAccountId,
  onManagedAccountSelect,
}: GeminiFormFieldsProps) {
  const { t } = useTranslation();
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const isGoogleOfficial =
    partnerPromotionKey?.toLowerCase() === "google-official";
  const isGeminiAutoProvider = managedAuthProvider === "gemini_auto";

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey)
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
  }, [apiKey, baseUrl, t]);

  return (
    <>
      {isGoogleOfficial && (
        <div className="space-y-4 rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950">
          <div className="flex gap-3">
            <Info className="h-5 w-5 flex-shrink-0 text-blue-600 dark:text-blue-400" />
            <div className="space-y-1">
              <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
                {t("provider.form.gemini.oauthTitle", {
                  defaultValue: "OAuth 认证模式",
                })}
              </p>
              <p className="text-sm text-blue-700 dark:text-blue-300">
                {t("provider.form.gemini.oauthHint", {
                  defaultValue:
                    "Google Official 使用 Google 账号 OAuth 托管认证，不需要手动填写 API Key。",
                })}
              </p>
            </div>
          </div>

          {isGeminiAutoProvider && (
            <GeminiAutoAuthSection
              selectedAccountId={selectedManagedAccountId}
              onAccountSelect={onManagedAccountSelect}
            />
          )}

          {isGeminiAutoProvider && !isManagedAuthAuthenticated && (
            <p className="text-sm text-amber-700 dark:text-amber-300">
              {t("provider.form.gemini.oauthLoginRequired", {
                defaultValue:
                  "请先完成 Google 账号登录，再保存 Google Official provider。",
              })}
            </p>
          )}
        </div>
      )}

      {shouldShowApiKey && !isGoogleOfficial && (
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

      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder", {
            defaultValue: "https://your-api-endpoint.com/",
          })}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {shouldShowModelField && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <FormLabel htmlFor="gemini-model">
              {t("provider.form.gemini.model", { defaultValue: "模型" })}
            </FormLabel>
            {!isGoogleOfficial && (
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
          <ModelInputWithFetch
            id="gemini-model"
            value={model}
            onChange={onModelChange}
            placeholder="gemini-2.5-pro"
            fetchedModels={fetchedModels}
            isLoading={isFetchingModels}
          />
        </div>
      )}

      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="gemini"
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
    </>
  );
}
