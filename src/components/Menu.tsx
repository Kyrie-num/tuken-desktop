import type { ReactNode } from "react";
import * as Menu from "@radix-ui/react-dropdown-menu";
import { cn } from "@/lib/cn";

export const MenuRoot = Menu.Root;
export const MenuTrigger = Menu.Trigger;

export function MenuContent({
  children,
  className,
  align = "end",
}: {
  children: ReactNode;
  className?: string;
  align?: "start" | "center" | "end";
}) {
  return (
    <Menu.Portal>
      <Menu.Content
        align={align}
        sideOffset={8}
        className={cn(
          "menu glass z-50 min-w-[220px] rounded-[18px] p-1.5 outline-none",
          className,
        )}
        // A sheet the item opened keeps the focus; back on the trigger, the
        // keyboard would be behind the sheet.
        onCloseAutoFocus={(e) => {
          if (document.querySelector('[role="dialog"]')) e.preventDefault();
        }}
      >
        {children}
      </Menu.Content>
    </Menu.Portal>
  );
}

const itemClass =
  "flex h-9 cursor-default select-none items-center gap-2.5 rounded-[12px] px-3 text-body outline-none data-[disabled]:opacity-40 data-[highlighted]:bg-ink/[0.08]";

export function MenuItem({
  children,
  onSelect,
  danger,
  disabled,
}: {
  children: ReactNode;
  onSelect?: () => void;
  danger?: boolean;
  disabled?: boolean;
}) {
  return (
    <Menu.Item
      className={cn(itemClass, danger && "text-danger")}
      onSelect={onSelect}
      disabled={disabled}
    >
      {children}
    </Menu.Item>
  );
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return (
    <Menu.Label className="px-3 pb-1 pt-2 text-caption font-semibold text-ink/50">
      {children}
    </Menu.Label>
  );
}

export function MenuSeparator() {
  return <Menu.Separator className="mx-2 my-1.5 h-px bg-ink/10" />;
}

export const MenuRadioGroup = Menu.RadioGroup;

export function MenuRadio({
  value,
  children,
}: {
  value: string;
  children: ReactNode;
}) {
  return (
    <Menu.RadioItem value={value} className={itemClass}>
      <span className="flex w-4 justify-center">
        <Menu.ItemIndicator>
          <span className="block h-1.5 w-1.5 rounded-full bg-cyan" />
        </Menu.ItemIndicator>
      </span>
      {children}
    </Menu.RadioItem>
  );
}
