import { createContext, useContext, type JSX } from "solid-js";
import type { YasWorkspace, TerminalPalette } from "@yas-run/core";

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
  children: JSX.Element;
}

export function YasWorkspaceProvider(props: YasProviderProps) {
  return (
    <YasContext.Provider
      value={{
        get workspace() {
          return props.workspace;
        },
        get palette() {
          return props.palette;
        },
        get fontFamily() {
          return props.fontFamily;
        },
        get fontSize() {
          return props.fontSize;
        },
        get advanceRatio() {
          return props.advanceRatio;
        },
        get textGamma() {
          return props.textGamma;
        },
      }}
    >
      {props.children}
    </YasContext.Provider>
  );
}

export function useRequiredYasWorkspace(): YasWorkspace {
  const ctx = useYasContext();
  if (!ctx.workspace) {
    throw new Error("YAS components require a YasWorkspaceProvider ancestor");
  }
  return ctx.workspace;
}
