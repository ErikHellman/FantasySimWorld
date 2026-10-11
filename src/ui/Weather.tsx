import type { Weather } from '../generated/api';
import { ui } from './store';

/** What the top bar says of the weather, or null under a clear sky. */
export function weatherLabel(weather: Weather): { icon: string; word: string } | null {
  switch (weather.kind) {
    case 'rain':
      return weather.intensity < 50 ? { icon: '🌦️', word: 'light rain' } : { icon: '🌧️', word: 'rain' };
    case 'wind':
      return weather.intensity < 70 ? { icon: '🍃', word: 'windy' } : { icon: '💨', word: 'gale' };
    case 'hail':
      return { icon: '🌨️', word: 'hail' };
    default:
      return null;
  }
}

/** The weather beside the clock, when there is any; on a narrow screen only its icon. */
export function WeatherWord() {
  const label = weatherLabel(ui.weather.value);
  if (!label) return null;
  return (
    <span class="clock-weather" title={`The weather: ${label.word}`}>
      {label.icon}
      <span class="clock-weather-word"> {label.word}</span>
    </span>
  );
}

const SEASON_ICON: Record<string, string> = { spring: '🌱', summer: '☀️', autumn: '🍂', winter: '❄️' };

/** The season and the year, beside the day: a year is four seasons of seven days. */
export function SeasonWord() {
  const season = ui.season.value;
  return (
    <span class="clock-season" title={`Year ${ui.year.value}: a year is four seasons of seven days`}>
      {SEASON_ICON[season] ?? ''} {season}
    </span>
  );
}
