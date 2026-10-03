import { useTranslation } from "react-i18next";
import { navigate, useRoute } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { PageShell } from "@/components/ui/PageShell/PageShell";

/** Fallback for an unknown hash. */
export function NotFoundPage() {
  const { t } = useTranslation();
  const route = useRoute();
  return (
    <PageShell title={t("page.notFound")} single>
      <EmptyState
        title={t("page.notFound")}
        detail={route.path !== "/not-found" ? route.path : undefined}
        actions={
          <Button size="sm" onClick={() => navigate("/dashboard")}>
            {t("nav.dashboard")}
          </Button>
        }
      />
    </PageShell>
  );
}
