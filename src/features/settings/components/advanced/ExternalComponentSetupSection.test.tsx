import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/lib/i18n";
import type {
  ExternalComponentSetupResult,
  ExternalComponentSetupStatus,
} from "@/rspc/bindings";
import { ExternalComponentSetupSection } from "./ExternalComponentSetupSection";

const mocks = vi.hoisted(() => ({
  error: vi.fn(),
  getExternalComponentSetupComponents: vi.fn(),
  getExternalComponentSetupStatus: vi.fn(),
  platform: vi.fn(() => "windows"),
  restartApp: vi.fn(),
  runExternalComponentSetup: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: mocks.platform,
}));

vi.mock("@/hooks/useTauriDialog", () => ({
  useTauriDialog: () => ({
    error: mocks.error,
  }),
}));

vi.mock("@/rspc/bindings", () => ({
  commands: {
    getExternalComponentSetupComponents:
      mocks.getExternalComponentSetupComponents,
    getExternalComponentSetupStatus: mocks.getExternalComponentSetupStatus,
    restartApp: mocks.restartApp,
    runExternalComponentSetup: mocks.runExternalComponentSetup,
  },
}));

const status = (
  overrides: Partial<ExternalComponentSetupStatus> = {},
): ExternalComponentSetupStatus => ({
  component: "pawnio",
  support: "supported",
  runtime: { installed: false, version: null, installLocation: null },
  moduleFiles: [
    { fileName: "IntelMSR.bin", present: false },
    { fileName: "RyzenSMU.bin", present: true },
    { fileName: "AMDFamily17.bin", present: false },
    { fileName: "LpcIO.bin", present: false },
  ],
  pinnedRuntimeVersion: "2.2.0",
  pinnedModulesVersion: "0.2.8",
  complete: false,
  ...overrides,
});

const completeStatus = (): ExternalComponentSetupStatus =>
  status({
    runtime: {
      installed: true,
      version: "2.2.0",
      installLocation: "C:\\Program Files\\PawnIO",
    },
    moduleFiles: status().moduleFiles.map((file) => ({
      ...file,
      present: true,
    })),
    complete: true,
  });

const result = (
  overrides: Partial<ExternalComponentSetupResult> = {},
): ExternalComponentSetupResult => ({
  component: "pawnio",
  outcome: "installed",
  detail: null,
  runtimeInstalled: true,
  moduleFilesPlaced: ["IntelMSR.bin", "AMDFamily17.bin", "LpcIO.bin"],
  status: completeStatus(),
  ...overrides,
});

describe("ExternalComponentSetupSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.platform.mockReturnValue("windows");
    mocks.getExternalComponentSetupComponents.mockResolvedValue(["pawnio"]);
    mocks.getExternalComponentSetupStatus.mockResolvedValue({
      status: "ok",
      data: status(),
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("renders nothing outside Windows", () => {
    mocks.platform.mockReturnValue("macos");

    const { container } = render(<ExternalComponentSetupSection />);

    expect(container).toBeEmptyDOMElement();
    expect(mocks.getExternalComponentSetupComponents).not.toHaveBeenCalled();
  });

  it("shows the runtime and module file state with an install action", async () => {
    render(<ExternalComponentSetupSection />);

    expect(
      await screen.findByText(
        "Runtime not installed (setup installs version 2.2.0)",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Module files: 1 of 4 present.", { exact: false }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Missing: IntelMSR.bin, AMDFamily17.bin, LpcIO.bin"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeEnabled();
  });

  it("disables the action when setup has nothing left to do", async () => {
    mocks.getExternalComponentSetupStatus.mockResolvedValue({
      status: "ok",
      data: completeStatus(),
    });

    render(<ExternalComponentSetupSection />);

    expect(
      await screen.findByText("Runtime installed (version 2.2.0)"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Installed" })).toBeDisabled();
  });

  it("runs setup, refreshes the state, and asks for a restart on success", async () => {
    const user = userEvent.setup();
    mocks.runExternalComponentSetup.mockResolvedValue({
      status: "ok",
      data: result(),
    });

    render(<ExternalComponentSetupSection />);

    await user.click(await screen.findByRole("button", { name: "Install" }));

    await waitFor(() => {
      expect(mocks.runExternalComponentSetup).toHaveBeenCalledWith("pawnio");
    });
    expect(
      await screen.findByText(
        "Restart HardwareVisualizer to start using the newly installed component.",
      ),
    ).toBeInTheDocument();
    // The restart dialog hides the page from the accessibility tree while open.
    expect(
      screen.getByText("Runtime installed (version 2.2.0)"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Installed", hidden: true }),
    ).toBeDisabled();
    expect(
      screen.getByText(
        "PawnIO was installed. Restart HardwareVisualizer to use the new sensors.",
      ),
    ).toBeInTheDocument();
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("reports a cancelled elevation prompt without an error dialog", async () => {
    const user = userEvent.setup();
    mocks.runExternalComponentSetup.mockResolvedValue({
      status: "ok",
      data: result({
        outcome: "cancelled",
        runtimeInstalled: false,
        moduleFilesPlaced: [],
        status: status(),
      }),
    });

    render(<ExternalComponentSetupSection />);

    await user.click(await screen.findByRole("button", { name: "Install" }));

    expect(
      await screen.findByText(
        "Installation was cancelled at the administrator prompt.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeEnabled();
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("shows the failure detail in an error dialog", async () => {
    const user = userEvent.setup();
    mocks.runExternalComponentSetup.mockResolvedValue({
      status: "ok",
      data: result({
        outcome: "failed",
        detail: "PawnIO_setup.exe SHA-256 mismatch",
        runtimeInstalled: false,
        moduleFilesPlaced: [],
        status: status(),
      }),
    });

    render(<ExternalComponentSetupSection />);

    await user.click(await screen.findByRole("button", { name: "Install" }));

    await waitFor(() => {
      expect(mocks.error).toHaveBeenCalledWith(
        "Installation failed: PawnIO_setup.exe SHA-256 mismatch",
      );
    });
  });

  it("surfaces a command error as a failure", async () => {
    const user = userEvent.setup();
    mocks.runExternalComponentSetup.mockResolvedValue({
      status: "error",
      error: "platform unavailable",
    });

    render(<ExternalComponentSetupSection />);

    await user.click(await screen.findByRole("button", { name: "Install" }));

    await waitFor(() => {
      expect(mocks.error).toHaveBeenCalledWith(
        "Installation failed: platform unavailable",
      );
    });
  });
});
