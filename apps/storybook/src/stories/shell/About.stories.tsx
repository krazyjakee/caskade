import type { Meta } from "@storybook/react-vite";
import { createContext, type MouseEvent, useContext, useState } from "react";
import { AboutPage } from "../../../../desktop/src/views/settings/about/AboutPage";
import { LegalView } from "../../../../desktop/src/views/settings/about/LegalView";
import type { LegalSection } from "../../../../desktop/src/views/settings/about/legal";
import {
  SectionLinkContext,
  type SectionLinkProps,
} from "../../../../desktop/src/views/settings/about/SectionLink";
import type { Story } from "../../catalogue/stories";

/** Settings → About and its Legal view (#162), the app's own components:
 * the links between them switch the story in place, as the router does in
 * the app. The Mode toolbar shows them in console mode too. */
const meta = { title: "Shell/About" } satisfies Meta;
export default meta;

/** Where a story's links go: the story's own section state. */
const OpenSection = createContext<(section: LegalSection | null) => void>(() => {});

function StoryLink({ section: to, ...props }: SectionLinkProps) {
  const setSection = useContext(OpenSection);
  const open = (event: MouseEvent) => {
    event.preventDefault();
    setSection(to);
  };
  return <a href={`#${to ?? "about"}`} onClick={open} {...props} />;
}

function About({ initial }: { initial: LegalSection | null }) {
  const [section, setSection] = useState(initial);
  return (
    <div className="max-w-4xl">
      <OpenSection value={setSection}>
        <SectionLinkContext value={StoryLink}>
          {section ? (
            <LegalView section={section} />
          ) : (
            <AboutPage version="0.1.0" build="Release build" />
          )}
        </SectionLinkContext>
      </OpenSection>
    </div>
  );
}

export const AboutDesktop: Story = { render: () => <About initial={null} /> };
export const AboutConsole: Story = {
  render: () => <About initial={null} />,
  globals: { mode: "console" },
};
export const Licenses: Story = { render: () => <About initial="licenses" /> };
export const Packages: Story = { render: () => <About initial="packages" /> };
export const Terms: Story = { render: () => <About initial="terms" /> };
export const Privacy: Story = { render: () => <About initial="privacy" /> };
export const PrivacyConsole: Story = {
  render: () => <About initial="privacy" />,
  globals: { mode: "console" },
};
