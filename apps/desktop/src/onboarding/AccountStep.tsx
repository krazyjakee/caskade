/** First-run setup's last question (#207): the admin's name, avatar and an
 * optional PIN. The PIN can stay empty while the admin is the only
 * account; it goes straight to the core and is never kept here. */
import { AvatarPicker, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";
import { type Account, useUpdateProfile, validPin } from "../api/accounts";
import { StepHeading } from "./Onboarding";
import { Continue } from "./Questions";

/** What the library's first admin is called until setup names them. */
const DEFAULT_NAME = "Player";

export function AccountStep({ account, onNext }: { account: Account; onNext(): void }) {
  const { t } = useLingui();
  // The first admin's stored name is English; offer it in the user's language
  // until they change it.
  const [name, setName] = useState(() =>
    account.name === DEFAULT_NAME
      ? t({ message: "Player", context: "account name" })
      : account.name,
  );
  const [avatar, setAvatar] = useState(account.avatar);
  const [pin, setPin] = useState("");
  const save = useUpdateProfile();
  const named = name.trim().length > 0;
  const pinOk = pin === "" || validPin(pin);
  return (
    <section data-part="question" data-question="account">
      <StepHeading
        title={t`Who are you?`}
        description={t`You're the admin: you'll add the others' accounts and choose what they can do.`}
      />
      <div className="flex animate-rise flex-col gap-6 [--i:2]">
        <TextField
          label={t({ message: "Name", context: "account" })}
          value={name}
          onChange={setName}
          maxLength={64}
          isInvalid={!named}
          validationBehavior="aria"
        />
        <div className="flex flex-col gap-2">
          <span id="onboarding-avatar" className="text-sm font-medium">
            <Trans>Avatar</Trans>
          </span>
          <AvatarPicker
            aria-labelledby="onboarding-avatar"
            value={avatar}
            onChange={setAvatar}
            size="md"
          />
        </div>
        <TextField
          label={t`PIN`}
          type="password"
          inputMode="numeric"
          autoComplete="off"
          maxLength={8}
          value={pin}
          onChange={(value) => setPin(value.replace(/\D/g, ""))}
          isInvalid={!pinOk}
          validationBehavior="aria"
          description={
            account.hasPin
              ? t`Your account has a PIN. Type a new one to change it.`
              : t`Optional while you're the only account: 4 to 8 digits that lock it.`
          }
        />
        {save.error && (
          <p role="alert" className="text-center text-sm text-danger">
            {save.error.message}
          </p>
        )}
      </div>
      <Continue
        isDisabled={!named || !pinOk || save.isPending}
        onPress={() =>
          save.mutate(
            { account, name: name.trim(), avatar, pin: pin || null },
            { onSuccess: () => onNext() },
          )
        }
      >
        <Trans comment="Finishes first-run setup's questions and starts the work">Set up</Trans>
      </Continue>
    </section>
  );
}
