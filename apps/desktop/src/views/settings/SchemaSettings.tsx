import { Select, SettingRow, SettingsSection, Slider, Switch } from "@caskade/ui";
import { t as translate } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import type { LucideIcon } from "lucide-react";
import { DraftField } from "./DraftField";
import { ResetDefaults } from "./ResetDefaults";
import {
  SettingsFeedback,
  SettingsLoading,
  useSettingsDefinition,
  useSettingsSection,
} from "./useSettings";

type ObjectSchema = Record<string, unknown>;
function object(value: unknown): ObjectSchema {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as ObjectSchema)
    : {};
}
function resolve(value: unknown, root: ObjectSchema, depth = 0): ObjectSchema {
  const schema = object(value);
  if (typeof schema.$ref !== "string") return schema;
  if (depth >= 8 || !schema.$ref.startsWith("#/$defs/"))
    throw new Error(translate`Unsupported settings schema reference.`);
  const name = schema.$ref.slice("#/$defs/".length);
  return { ...resolve(object(root.$defs)[name], root, depth + 1), ...schema };
}
function label(value: string) {
  return value.replaceAll("_", " ").replace(/^./, (letter) => letter.toUpperCase());
}
function options(schema: ObjectSchema, root: ObjectSchema) {
  if (Array.isArray(schema.enum))
    return schema.enum
      .filter((value): value is string => typeof value === "string")
      .map((value) => ({ id: value, label: label(value) }));
  const variants = Array.isArray(schema.oneOf)
    ? schema.oneOf
    : Array.isArray(schema.anyOf)
      ? schema.anyOf
      : [];
  return variants.flatMap((value) => {
    const item = resolve(value, root);
    const id =
      typeof item.const === "string"
        ? item.const
        : Array.isArray(item.enum) && item.enum.length === 1 && typeof item.enum[0] === "string"
          ? item.enum[0]
          : undefined;
    return id === undefined
      ? []
      : [{ id, label: typeof item.title === "string" ? item.title : label(id) }];
  });
}

/** A settings section rendered from its JSON Schema (#59, #128): controls,
 * choices, labels and limits come from core, which validates every save and
 * translates the schema's titles and descriptions (#214). */
export function SchemaSettings({
  namespace,
  title,
  description,
  icon,
  resetLabel,
}: {
  namespace: string;
  title: string;
  description?: string;
  icon?: LucideIcon;
  resetLabel?: string;
}) {
  const { t } = useLingui();
  const definition = useSettingsDefinition(namespace);
  const { settings, setSettings, saving, error, reload } =
    useSettingsSection<Record<string, unknown>>(namespace);
  if (!settings || !definition.data)
    return (
      <SettingsLoading
        error={error ?? definition.error}
        reload={() => {
          reload();
          void definition.refetch();
        }}
      />
    );
  const root = object(definition.data.schema);
  let fields: [string, ObjectSchema][];
  try {
    fields = Object.entries(object(root.properties)).map(([key, value]) => [
      key,
      resolve(value, root),
    ]);
  } catch (problem) {
    return (
      <p role="alert">
        {problem instanceof Error ? problem.message : t`These settings are unavailable.`}
      </p>
    );
  }
  const set = (key: string, value: unknown) =>
    setSettings((current) => ({ ...current, [key]: value }));
  return (
    <div data-settings={namespace} data-cs="schema-settings" className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-3">
        <SettingsFeedback error={error} saving={saving} />
        <ResetDefaults
          group={title}
          label={resetLabel ?? t`Reset ${{ section: title }} settings`}
          parts={[
            {
              namespace,
              whole: true,
              fields: fields.map(([key, schema]) => ({
                key,
                label: typeof schema.title === "string" ? schema.title : label(key),
              })),
            },
          ]}
          isDisabled={saving}
        />
      </div>
      <SettingsSection icon={icon} title={title} description={description}>
        {fields.map(([key, schema]) => {
          const title = typeof schema.title === "string" ? schema.title : label(key);
          const description =
            typeof schema.description === "string" ? schema.description : undefined;
          const choices = options(schema, root);
          const value = settings[key];
          return (
            <SettingRow key={key} label={title} description={description}>
              {choices.length && typeof value === "string" ? (
                <Select
                  options={choices}
                  value={value}
                  onChange={(next) => set(key, next)}
                  className="w-48"
                />
              ) : schema.type === "boolean" && typeof value === "boolean" ? (
                <Switch isSelected={value} onChange={(next) => set(key, next)} />
              ) : (schema.type === "number" || schema.type === "integer") &&
                typeof value === "number" &&
                typeof schema.minimum === "number" &&
                typeof schema.maximum === "number" ? (
                <Slider
                  value={value}
                  minValue={schema.minimum}
                  maxValue={schema.maximum}
                  step={schema.type === "integer" ? 1 : 0.05}
                  onChange={(next) => set(key, next)}
                  className="w-48"
                />
              ) : (schema.type === "number" || schema.type === "integer") &&
                typeof value === "number" ? (
                <DraftField
                  value={String(value)}
                  inputMode="decimal"
                  commit={(next) => {
                    const parsed = Number(next);
                    if (next.trim() && Number.isFinite(parsed)) set(key, parsed);
                  }}
                  className="w-48"
                />
              ) : schema.type === "string" && typeof value === "string" ? (
                <DraftField
                  value={value}
                  commit={(next) => set(key, next)}
                  maxLength={typeof schema.maxLength === "number" ? schema.maxLength : undefined}
                  className="w-48"
                />
              ) : (
                <span role="alert" className="text-sm text-danger">
                  <Trans>Unsupported setting type.</Trans>
                </span>
              )}
            </SettingRow>
          );
        })}
      </SettingsSection>
    </div>
  );
}
