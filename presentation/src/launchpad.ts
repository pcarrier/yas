export interface LaunchpadDeckState {
  slideIndex: number;
  slideMidiColors: readonly number[];
  sectionStarts: readonly number[];
  sectionMidiColors: readonly number[];
}

export interface LaunchpadActions {
  goToSlide(index: number): void;
  goToSection(index: number): void;
  status(message: string, connected: boolean): void;
}

const HEADER = [0xf0, 0x00, 0x20, 0x29, 0x02, 0x0d];
const DAW_MODE = [...HEADER, 0x10, 0x01, 0xf7];
const STANDALONE_MODE = [...HEADER, 0x10, 0x00, 0xf7];
const SESSION_LAYOUT = [...HEADER, 0x00, 0x00, 0xf7];

// DAW Session layout: top grid row is 81..88, bottom is 11..18.
export const PAD_NOTES = Array.from({ length: 64 }, (_, index) => {
  const rowFromTop = Math.floor(index / 8);
  const column = index % 8;
  return (8 - rowFromTop) * 10 + column + 1;
});

const SLIDE_PAD_NOTES = PAD_NOTES.slice(0, 56);
const SECTION_PAD_NOTES = PAD_NOTES.slice(56);

const OFF = 0;
const DIM = 1;
const WHITE = 3;
function isLaunchpad(name: string): boolean {
  return /launchpad\s*mini|lpminimk3/i.test(name);
}

function portScore(name: string): number {
  const lower = name.toLowerCase();
  return lower.includes("daw") ? 10 : 0;
}

function bestPort<T extends MIDIInput | MIDIOutput>(ports: Iterable<T>): T | null {
  return [...ports]
    .filter((port) => port.state === "connected" && isLaunchpad(port.name ?? ""))
    .sort((left, right) => portScore(right.name ?? "") - portScore(left.name ?? ""))[0] ?? null;
}

export class LaunchpadMiniMk3 {
  private access: MIDIAccess | null = null;
  private input: MIDIInput | null = null;
  private output: MIDIOutput | null = null;
  private state: LaunchpadDeckState | null = null;

  constructor(private readonly actions: LaunchpadActions) {}

  get connected(): boolean {
    return this.input?.state === "connected" && this.output?.state === "connected";
  }

  async connect(): Promise<void> {
    if (!("requestMIDIAccess" in navigator)) {
      this.actions.status("Web MIDI is unavailable in this browser", false);
      return;
    }

    this.actions.status("Requesting MIDI access…", false);
    try {
      this.access = await navigator.requestMIDIAccess({ sysex: true });
    } catch {
      this.actions.status("MIDI + SysEx permission was not granted", false);
      return;
    }

    this.access.onstatechange = () => this.bindPorts();
    this.bindPorts();
  }

  disconnect(): void {
    this.restoreStandaloneMode();
    if (this.input) this.input.onmidimessage = null;
    if (this.access) this.access.onstatechange = null;
    this.input = null;
    this.output = null;
    this.actions.status("Launchpad disconnected", false);
  }

  update(state: LaunchpadDeckState): void {
    this.state = state;
    this.paint();
  }

  restoreStandaloneMode(): void {
    if (!this.output) return;
    PAD_NOTES.forEach((note) => this.note(note, OFF));
    this.output.send(STANDALONE_MODE);
  }

  private bindPorts(): void {
    if (!this.access) return;
    if (this.input) this.input.onmidimessage = null;
    this.input = bestPort(this.access.inputs.values());
    this.output = bestPort(this.access.outputs.values());
    if (!this.input || !this.output) {
      this.actions.status("Connect the Launchpad Mini Mk3 DAW port", false);
      return;
    }
    this.input.onmidimessage = (event) => this.onMidi(event);
    this.output.send(DAW_MODE);
    this.output.send(SESSION_LAYOUT);
    this.actions.status("Launchpad Mini Mk3 · DAW / Session", true);
    this.paint();
  }

  private onMidi(event: MIDIMessageEvent): void {
    if (!event.data) return;
    const [status = 0, number = 0, value = 0] = event.data;
    if (value === 0) return;
    const kind = status & 0xf0;
    if (kind === 0x90) {
      const section = SECTION_PAD_NOTES.indexOf(number);
      if (section >= 0) {
        this.actions.goToSection(section);
        return;
      }
      const slide = SLIDE_PAD_NOTES.indexOf(number);
      if (slide >= 0 && slide < (this.state?.slideMidiColors.length ?? 0)) {
        this.actions.goToSlide(slide);
      }
    }
  }

  private paint(): void {
    if (!this.output || !this.state) return;
    const state = this.state;
    for (let index = 0; index < SLIDE_PAD_NOTES.length; index += 1) {
      const color = index < state.slideMidiColors.length
        ? state.slideMidiColors[index] ?? DIM
        : OFF;
      this.note(SLIDE_PAD_NOTES[index], index === state.slideIndex ? WHITE : color);
    }
    for (let index = 0; index < SECTION_PAD_NOTES.length; index += 1) {
      const start = state.sectionStarts[index];
      const nextStart = state.sectionStarts[index + 1] ?? Number.POSITIVE_INFINITY;
      const active = state.slideIndex >= start && state.slideIndex < nextStart;
      this.note(SECTION_PAD_NOTES[index], active ? WHITE : (state.sectionMidiColors[index] ?? DIM));
    }
  }

  private note(note: number, color: number): void {
    this.output?.send([0x90, note, color]);
  }
}
