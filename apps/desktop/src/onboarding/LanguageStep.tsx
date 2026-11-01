/** First-run setup's first question (#213): the language, System (the OS's)
 * to start with. A choice saves `core.general.language`, and the core
 * switches the app at once, so the rest of setup is in it. */
import { useLingui } from "@lingui/react/macro";
import { Check } from "lucide-react";
import { Radio, RadioGroup } from "react-aria-components";
import { SYSTEM } from "../api/locale";
import { useLanguageOptions } from "../locale/LocaleProvider";
import { SettingsFeedback, useSettingsSection } from "../views/settings/useSettings";
import { StepHeading } from "./Onboarding";
import { Continue } from "./Questions";

export function LanguageStep({ onNext }: { onNext(): void }) {
  const { t } = useLingui();
  const general = useSettingsSection<{ language: string }>("core.general");
  const value = general.settings?.language ?? SYSTEM;
  const options = useLanguageOptions(value);
  return (
    <section data-part="question" data-question="language">
      <StepHeading
        title={t`Choose your language`}
        description={t`Caskade follows your system's language unless you pick another. You can change it later in Settings.`}
      />
      <RadioGroup
        aria-label={t`Language`}
        value={value}
        isDisabled={!general.settings}
        onChange={(language) => general.setSettings((current) => ({ ...current, language }))}
        className="mx-auto flex max-w-md flex-col gap-2"
      >
        {options.map((option, index) => (
          <Radio
            key={option.id}
            value={option.id}
            data-part="language"
            style={{ "--i": index + 3 } as React.CSSProperties}
            className="group flex animate-rise cursor-default items-center justify-between gap-3 rounded-panel border border-border-strong bg-surface-raised px-4 py-3 transition-[border-color,box-shadow,background-color] duration-(--cs-motion-duration-base) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-selected:border-accent data-selected:shadow-[0_0_20px_-8px] data-selected:shadow-glow"
          >
            <span className="font-medium">{option.label}</span>
            <Check
              aria-hidden
              className="size-4 text-accent-alt opacity-0 group-data-selected:opacity-100"
            />
          </Radio>
        ))}
      </RadioGroup>
      <div className="mt-3">
        <SettingsFeedback error={general.error} saving={false} />
      </div>
      <Continue onPress={onNext} />
    </section>
  );
}
