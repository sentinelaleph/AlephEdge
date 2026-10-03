import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { Panel } from "@/components/ui/Panel/Panel";
import { collapseTransition } from "@/lib/motion";
import { AddKeyForm } from "../AddKeyForm/AddKeyForm";
import { KeyList } from "../KeyList/KeyList";
import type { VaultController } from "../useVault";
import "./VaultManager.css";

interface VaultManagerProps {
  vault: VaultController;
}

/**
 * The unlocked vault's center content: stored keys first, the add-key form on
 * demand. The form starts open only while the vault is empty; once a key
 * exists it folds away behind "Add key". Vault state (lock, auto-lock, key
 * count) is the page's left summary and Lock is a header action, so nothing
 * here repeats them.
 */
export function VaultManager({ vault }: VaultManagerProps) {
  const { t } = useTranslation();
  const count = vault.credentials.length;
  const [adding, setAdding] = useState(count === 0);
  const prevCount = useRef(count);

  // Fold the form after a key was added; reopen it when the last key goes.
  useEffect(() => {
    if (count > prevCount.current) setAdding(false);
    if (count === 0) setAdding(true);
    prevCount.current = count;
  }, [count]);

  return (
    <div className="ae-vaultmgr">
      <Panel
        title={t("vault.keysTitle")}
        aside={
          count > 0 && !adding ? (
            <Button variant="secondary" size="sm" onClick={() => setAdding(true)} aria-expanded={adding}>
              {t("vault.addKey")}
            </Button>
          ) : null
        }
      >
        <KeyList
          credentials={vault.credentials}
          busy={vault.busy}
          error={vault.error}
          onRemove={vault.removeKey}
          onRenew={vault.renewKey}
          onRename={vault.renameKey}
        />
      </Panel>

      <AnimatePresence initial={false}>
        {adding ? (
          <motion.div
            key="add"
            className="ae-vaultmgr__add"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={collapseTransition()}
          >
            <AddKeyForm
              onAdd={vault.addKey}
              busy={vault.busy}
              error={vault.error}
              onCancel={count > 0 ? () => setAdding(false) : undefined}
            />
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
