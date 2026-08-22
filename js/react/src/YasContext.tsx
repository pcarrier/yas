import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { TerminalPalette } from "@yas-run/core";
import type { YasWorkspace } from "@yas-run/core";

export interface YasContextValue {
  workspace?: YasWorkspace;
  palette?: TerminalPalette;
  fontFamily?: string;
  fontSize?: number;
  advanceRatio?: number;
  textGamma?: number;
}

const YasContext = createContext<YasContextValue>({});

export function useYasContext(): YasContextValue {
  return useContext(YasContext);
}

export interface YasProviderProps extends YasContextValue {
  children: ReactNode;
}

export function YasWorkspaceProvider({
  children,
  workspace,
  palette,
  fontFamily,
  fontSize,
  advanceRatio,
  textGamma,
}: YasProviderProps) {
  const value = useMemo(
    () => ({
      workspace,
      palette,
      fontFamily,
      fontSize,
      advanceRatio,
      textGamma,
    }),
    [workspace, palette, fontFamily, fontSize, advanceRatio, textGamma],
  );
  return <YasContext.Provider value={value}>{children}</YasContext.Provider>;
}

export function useRequiredYasWorkspace(): YasWorkspace {
  const workspace = useYasContext().workspace;
  if (!workspace) {
    throw new Error("YAS components require a YasWorkspaceProvider ancestor");
  }
  return workspace;
}
