import { useEffect, useRef, type RefObject } from "react";

import { getSettingValue, useAppSettingsStore } from "../../store/useAppSettingsStore";
import { bindEqPersistence, useEqStore } from "../../store/useEqStore";
import { SETTINGS } from "../settings/schema";
import { REFERENCE_SAMPLE_RATE } from "./eq/filterMath";
import { graphChangesSound, graphSpec } from "./eq/graphPlan";
import { changesSound } from "./eq/stateOps";
import { MediaEqEngine } from "./mediaEqEngine";

/** The saved volume boost as a gain: 1 when off. */
function boostFactor(): number {
  const percent = Number(getSettingValue(SETTINGS.VOLUME_BOOST_PERCENT));
  return getSettingValue(SETTINGS.ALLOW_VOLUME_BOOST) === "true" && Number.isFinite(percent) ? percent / 100 : 1;
}

/**
 * Applies the shared equalizer to one video player. `audioKey` changes whenever the separate
 * adaptive-playback <audio> element mounts, unmounts or changes, so the engine follows it.
 */
export function useVideoEq(
  videoRef: RefObject<HTMLVideoElement | null>,
  audioRef: RefObject<HTMLAudioElement | null>,
  audioKey: string | null,
): void {
  const engineRef = useRef<MediaEqEngine | null>(null);

  useEffect(() => {
    // The pop-out window is its own JS context: it reads the equalizer from its settings.
    bindEqPersistence();
    const engine = new MediaEqEngine();
    engineRef.current = engine;
    const apply = () => {
      const { state, bypass, preview } = useEqStore.getState();
      const engaged =
        changesSound(state) ||
        (preview !== null && graphChangesSound(graphSpec(state, false, preview), REFERENCE_SAMPLE_RATE));
      engine.setEq(graphSpec(state, bypass, preview), engaged);
    };
    apply();
    const applyBoost = () => engine.setBoost(boostFactor());
    applyBoost();
    const unsubscribe = useEqStore.subscribe((s, previous) => {
      if (s.state !== previous.state || s.bypass !== previous.bypass || s.preview !== previous.preview) apply();
    });
    const unsubscribeSettings = useAppSettingsStore.subscribe((s, previous) => {
      if (s.values !== previous.values) applyBoost();
    });
    return () => {
      unsubscribe();
      unsubscribeSettings();
      engine.dispose();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => {
    engineRef.current?.setElements([videoRef.current, audioRef.current]);
  }, [videoRef, audioRef, audioKey]);
}
