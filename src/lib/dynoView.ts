/** Параметры Virtual Dyno из config page (см. dynoChars в INI). */
export interface DynoConfig {
  dynoRpmStep: number;
  dynoSaeTemperatureC: number;
  dynoSaeRelativeHumidity: number;
  dynoSaeBaro: number;
  dynoCarWheelDiaInch: number;
  dynoCarWheelAspectRatio: number;
  dynoCarWheelTireWidthMm: number;
  dynoCarGearPrimaryReduction: number;
  dynoCarGearRatio: number;
  dynoCarGearFinalDrive: number;
  dynoCarCarMassKg: number;
  dynoCarCargoMassKg: number;
  dynoCarCoeffOfDrag: number;
  dynoCarFrontalAreaM2: number;
}

export interface DynoPoint {
  rpm: number;
  time: number;
  tps: number;
  engineRps: number;
  axleRps: number;
  vMs: number;
  mph: number;
  distanceM: number;
  aMs2: number;
  forceN: number;
  forceDragN: number;
  forceTotalN: number;
  torqueWheelNm: number;
  torqueNm: number;
  torqueLbFt: number;
  hp: number;
}

export interface DynoRunPoint {
  rpm: number;
  torqueNm: number;
  hp: number;
}

const DYNO_VIEW_WINDOW_SIZE = 7;
const DYNO_VIEW_WINDOW_SIZE_RPM = 10;
const DYNO_VIEW_TPS_MIN_FOR_RUN = 30;
const DYNO_VIEW_RPM_DIFF_SMOOTH = 30;
const DYNO_VIEW_LOG_TIME_SMOOTH_SEC = 0.05;
const DYNO_VIEW_TPS_DIFF_TO_RESET_RUN = 10;
const DYNO_VIEW_RPM_FALL_TO_RESET_RUN = 60;

function move(size: number, data: Float64Array): void {
  for (let i = size - 1; i > 0; i -= 1) {
    data[i] = data[i - 1]!;
  }
}

function accumulateWindow(size: number, data: Float64Array): number {
  let sum = 0;
  for (let i = 0; i < size; i += 1) {
    sum += data[size - i - 1]!;
  }
  return sum / size;
}

/** Порт `DynoView` из virtualdyno-c++. */
export class DynoView {
  currentTorque = 0;
  currentHP = 0;

  private readonly airDensityKgM3 = 1.225;
  private wheelOverallDiameterMm = 0;
  private saeBaroCorrectionFactor = 1;
  private saeBaroMmhg = 0;
  private saeTempCorrectionFactor = 1;
  private saeVaporPressure = 0;
  private saeCorrectionFactor = 1;

  private dynoViewPoint: DynoPoint = DynoView.emptyPoint();
  private dynoViewPointPrev: DynoPoint = DynoView.emptyPoint();

  private count = 0;
  private countRpm = 0;
  private prevRpm = 0;

  private readonly tailHp = new Float64Array(DYNO_VIEW_WINDOW_SIZE);
  private readonly tailTorque = new Float64Array(DYNO_VIEW_WINDOW_SIZE);
  private readonly tailRpm = new Float64Array(DYNO_VIEW_WINDOW_SIZE_RPM);

  private initialized = false;

  constructor(private config: DynoConfig) {
    this.init();
  }

  static emptyPoint(): DynoPoint {
    return {
      rpm: -1,
      time: -1,
      tps: -1,
      engineRps: 0,
      axleRps: 0,
      vMs: 0,
      mph: 0,
      distanceM: 0,
      aMs2: 0,
      forceN: 0,
      forceDragN: 0,
      forceTotalN: 0,
      torqueWheelNm: 0,
      torqueNm: 0,
      torqueLbFt: 0,
      hp: 0,
    };
  }

  updateConfig(config: DynoConfig): void {
    this.config = config;
    this.initialized = false;
    this.init();
  }

  init(): void {
    if (this.initialized) return;
    this.initialized = true;

    const c = this.config;
    this.wheelOverallDiameterMm = Math.round(
      c.dynoCarWheelDiaInch * 25.4 +
        c.dynoCarWheelTireWidthMm * c.dynoCarWheelAspectRatio * 0.01 * 2.0,
    );

    this.saeVaporPressure =
      6.1078 *
      10 ** ((7.5 * c.dynoSaeTemperatureC) / (237.3 + c.dynoSaeTemperatureC)) *
      0.02953 *
      (c.dynoSaeRelativeHumidity / 100.0);

    this.saeBaroMmhg = 29.23 * (c.dynoSaeBaro / 100.0);
    this.saeBaroCorrectionFactor = 29.23 / (this.saeBaroMmhg - this.saeVaporPressure);
    this.saeTempCorrectionFactor = ((c.dynoSaeTemperatureC + 273.0) / 298.0) ** 0.5;
    this.saeCorrectionFactor =
      1.176 * (this.saeBaroCorrectionFactor * this.saeTempCorrectionFactor) - 0.176;

    this.reset();
  }

  reset(): void {
    this.dynoViewPointPrev = DynoView.emptyPoint();
    this.count = 0;
    this.countRpm = 0;
    this.currentTorque = 0;
    this.currentHP = 0;
    this.tailHp.fill(0);
    this.tailTorque.fill(0);
    this.tailRpm.fill(0);
  }

  /** @returns точка кривой или null, если сэмпл отфильтрован */
  onRpm(rpm: number, time: number, tps: number): DynoRunPoint | null {
    if (
      tps < DYNO_VIEW_TPS_MIN_FOR_RUN ||
      this.dynoViewPointPrev.tps - tps > DYNO_VIEW_TPS_DIFF_TO_RESET_RUN
    ) {
      this.reset();
      return null;
    }

    if (this.dynoViewPointPrev.rpm > 0 && this.dynoViewPointPrev.time > 0) {
      if (Math.abs(rpm - this.prevRpm) < 1) {
        return null;
      }
      this.prevRpm = rpm;

      if (time - this.dynoViewPointPrev.time < DYNO_VIEW_LOG_TIME_SMOOTH_SEC) {
        return null;
      }

      const rpmDiffSmooth = Math.abs(rpm - this.dynoViewPointPrev.rpm);
      if (rpmDiffSmooth < DYNO_VIEW_RPM_DIFF_SMOOTH) {
        return null;
      }

      move(DYNO_VIEW_WINDOW_SIZE_RPM, this.tailRpm);
      this.tailRpm[0] = rpm;

      this.countRpm += 1;
      const accumulateRpmSize = Math.min(this.countRpm, DYNO_VIEW_WINDOW_SIZE_RPM);
      this.dynoViewPoint.rpm = Math.round(accumulateWindow(accumulateRpmSize, this.tailRpm));

      if (
        this.dynoViewPoint.rpm + DYNO_VIEW_RPM_FALL_TO_RESET_RUN <
        this.dynoViewPointPrev.rpm
      ) {
        this.reset();
        return null;
      }

      const rpmDiffStep = Math.abs(this.dynoViewPoint.rpm - this.dynoViewPointPrev.rpm);
      if (rpmDiffStep < this.config.dynoRpmStep) {
        return null;
      }
    } else {
      this.dynoViewPoint.rpm = rpm;
    }

    this.dynoViewPoint.time = time;
    this.dynoViewPoint.tps = tps;

    const gear =
      this.config.dynoCarGearPrimaryReduction *
      this.config.dynoCarGearRatio *
      this.config.dynoCarGearFinalDrive;

    this.dynoViewPoint.engineRps = this.dynoViewPoint.rpm / 60.0;
    this.dynoViewPoint.axleRps = this.dynoViewPoint.engineRps / gear;
    this.dynoViewPoint.vMs =
      this.dynoViewPoint.axleRps * (this.wheelOverallDiameterMm / 1000.0) * 3.1416;
    this.dynoViewPoint.mph = this.dynoViewPoint.vMs * 2.2369363;

    if (this.dynoViewPointPrev.rpm > 0 && this.dynoViewPointPrev.time > 0) {
      const dt = this.dynoViewPoint.time - this.dynoViewPointPrev.time;
      this.dynoViewPoint.distanceM =
        ((this.dynoViewPoint.vMs + this.dynoViewPointPrev.vMs) / 2.0) * dt;
      this.dynoViewPoint.aMs2 = (this.dynoViewPoint.vMs - this.dynoViewPointPrev.vMs) / dt;
      if (this.dynoViewPoint.aMs2 < 0) {
        this.dynoViewPoint.aMs2 = 0;
      }

      this.dynoViewPoint.forceN =
        (this.config.dynoCarCargoMassKg + this.config.dynoCarCarMassKg) *
        this.dynoViewPoint.aMs2;

      this.dynoViewPoint.forceDragN =
        0.5 *
        this.airDensityKgM3 *
        (this.dynoViewPoint.vMs * this.dynoViewPoint.vMs) *
        this.config.dynoCarFrontalAreaM2 *
        this.config.dynoCarCoeffOfDrag;

      this.dynoViewPoint.forceDragN *= this.saeCorrectionFactor;

      this.dynoViewPoint.forceTotalN =
        this.dynoViewPoint.forceN + this.dynoViewPoint.forceDragN;
      this.dynoViewPoint.torqueWheelNm =
        this.dynoViewPoint.forceTotalN * ((this.wheelOverallDiameterMm / 2.0) / 1000.0);
      this.dynoViewPoint.torqueNm = this.dynoViewPoint.torqueWheelNm / gear;
      this.dynoViewPoint.torqueLbFt = this.dynoViewPoint.torqueNm * 0.737562;
      this.dynoViewPoint.hp = (this.dynoViewPoint.torqueLbFt * this.dynoViewPoint.rpm) / 5252.0;

      move(DYNO_VIEW_WINDOW_SIZE, this.tailHp);
      move(DYNO_VIEW_WINDOW_SIZE, this.tailTorque);

      this.tailTorque[0] = this.dynoViewPoint.torqueNm;
      this.tailHp[0] = this.dynoViewPoint.hp;

      if (this.count < DYNO_VIEW_WINDOW_SIZE) {
        this.count += 1;
      }

      const accumulateSize = Math.min(this.count, DYNO_VIEW_WINDOW_SIZE);
      this.currentTorque = accumulateWindow(accumulateSize, this.tailTorque);
      this.currentHP = accumulateWindow(accumulateSize, this.tailHp);

      this.dynoViewPointPrev = { ...this.dynoViewPoint };
      return {
        rpm: this.dynoViewPoint.rpm,
        torqueNm: this.currentTorque,
        hp: this.currentHP,
      };
    }

    this.dynoViewPointPrev = { ...this.dynoViewPoint };
    return null;
  }
}

export function dynoConfigFromValues(
  get: (name: string) => number | null,
  fallback: DynoConfig = DEFAULT_DYNO_CONFIG,
): DynoConfig {
  const num = (name: keyof DynoConfig): number => {
    const v = get(name);
    return v === null || !Number.isFinite(v) ? fallback[name] : v;
  };
  return {
    dynoRpmStep: num("dynoRpmStep"),
    dynoSaeTemperatureC: num("dynoSaeTemperatureC"),
    dynoSaeRelativeHumidity: num("dynoSaeRelativeHumidity"),
    dynoSaeBaro: num("dynoSaeBaro"),
    dynoCarWheelDiaInch: num("dynoCarWheelDiaInch"),
    dynoCarWheelAspectRatio: num("dynoCarWheelAspectRatio"),
    dynoCarWheelTireWidthMm: num("dynoCarWheelTireWidthMm"),
    dynoCarGearPrimaryReduction: num("dynoCarGearPrimaryReduction"),
    dynoCarGearRatio: num("dynoCarGearRatio"),
    dynoCarGearFinalDrive: num("dynoCarGearFinalDrive"),
    dynoCarCarMassKg: num("dynoCarCarMassKg"),
    dynoCarCargoMassKg: num("dynoCarCargoMassKg"),
    dynoCarCoeffOfDrag: num("dynoCarCoeffOfDrag"),
    dynoCarFrontalAreaM2: num("dynoCarFrontalAreaM2"),
  };
}

export const DEFAULT_DYNO_CONFIG: DynoConfig = {
  dynoRpmStep: 100,
  dynoSaeTemperatureC: 20,
  dynoSaeRelativeHumidity: 80,
  dynoSaeBaro: 101.33,
  dynoCarWheelDiaInch: 18,
  dynoCarWheelAspectRatio: 55,
  dynoCarWheelTireWidthMm: 180,
  dynoCarGearPrimaryReduction: 1,
  dynoCarGearRatio: 1,
  dynoCarGearFinalDrive: 3.5,
  dynoCarCarMassKg: 1200,
  dynoCarCargoMassKg: 80,
  dynoCarCoeffOfDrag: 0.32,
  dynoCarFrontalAreaM2: 2.2,
};
