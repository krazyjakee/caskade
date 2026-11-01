import { Select } from "@caskade/ui";
import { useLanguageOptions } from "./LocaleProvider";

/** The Language setting's value (`core.general.language`): System or a
 * language tag. Saving it switches the app through the core. */
export function LanguageSelect({
  value,
  onChange,
  className = "w-56",
}: {
  value: string;
  onChange(value: string): void;
  className?: string;
}) {
  const options = useLanguageOptions(value);
  return <Select options={options} value={value} onChange={onChange} className={className} />;
}
