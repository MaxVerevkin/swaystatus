use std::time::Duration;

use serde_derive::Deserialize;

use super::prelude::*;
use crate::de::deserialize_duration;

const IP_API_URL: &str = "https://ipapi.co/json";

const OPEN_WEATHER_MAP_URL: &str = "https://api.openweathermap.org/data/2.5/weather";
const OPEN_WEATHER_MAP_API_KEY_ENV: &str = "OPENWEATHERMAP_API_KEY";
const OPEN_WEATHER_MAP_CITY_ID_ENV: &str = "OPENWEATHERMAP_CITY_ID";
const OPEN_WEATHER_MAP_PLACE_ENV: &str = "OPENWEATHERMAP_PLACE";

#[derive(Deserialize)]
struct ApiResponse {
    weather: Vec<ApiWeather>,
    main: ApiMain,
    wind: ApiWind,
    name: String,
}

#[derive(Deserialize)]
struct ApiWind {
    speed: f64,
    deg: Option<f64>,
}

#[derive(Deserialize)]
struct ApiMain {
    temp: f64,
    humidity: f64,
}

#[derive(Deserialize)]
struct ApiWeather {
    main: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "name", rename_all = "lowercase")]
enum WeatherService {
    OpenWeatherMap {
        api_key: Option<String>,
        city_id: Option<String>,
        place: Option<String>,
        coordinates: Option<(String, String)>,
        #[serde(default)]
        units: OpenWeatherMapUnits,
    },
}

impl WeatherService {
    fn units(&self) -> OpenWeatherMapUnits {
        let Self::OpenWeatherMap { units, .. } = self;
        *units
    }

    // FIXME use `autolocate`
    async fn get(&self, _autolocate: bool) -> Result<ApiResponse> {
        let Self::OpenWeatherMap {
            api_key,
            city_id,
            place,
            coordinates,
            units,
        } = self;

        let api_key = api_key.as_ref().error(format!(
            "missing key 'service.api_key' and environment variable {}",
            OPEN_WEATHER_MAP_API_KEY_ENV.to_string()
        ))?;

        let city = find_ip_location().await?;
        let location_query = {
            city.map(|x| format!("q={}", x))
                .or_else(|| city_id.as_ref().map(|x| format!("id={}", x)))
                .or_else(|| place.as_ref().map(|x| format!("q={}", x)))
                .or_else(|| {
                    coordinates
                        .as_ref()
                        .map(|(lat, lon)| format!("lat={}&lon={}", lat, lon))
                })
                .error("no localization was provided")?
        };

        // Refer to https://openweathermap.org/current
        let url = &format!(
            "{}?{}&appid={}&units={}",
            OPEN_WEATHER_MAP_URL,
            location_query,
            api_key,
            match *units {
                OpenWeatherMapUnits::Metric => "metric",
                OpenWeatherMapUnits::Imperial => "imperial",
            },
        );

        reqwest::get(url)
            .await
            .error("failed during request for current location")?
            .json()
            .await
            .error("failed while parsing location API result")
    }
}

impl Default for WeatherService {
    fn default() -> Self {
        Self::OpenWeatherMap {
            api_key: Some(OPEN_WEATHER_MAP_API_KEY_ENV.into()),
            city_id: Some(OPEN_WEATHER_MAP_CITY_ID_ENV.into()),
            place: Some(OPEN_WEATHER_MAP_PLACE_ENV.into()),
            coordinates: None,
            units: OpenWeatherMapUnits::Metric,
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum OpenWeatherMapUnits {
    Metric,
    Imperial,
}

impl Default for OpenWeatherMapUnits {
    fn default() -> Self {
        Self::Metric
    }
}

async fn find_ip_location() -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct ApiResponse {
        city: Option<String>,
    }

    let res: ApiResponse = dbg!(reqwest::get(IP_API_URL).await)
        .error("failed during request for current location")?
        .json()
        .await
        .error("failed while parsing location API result")?;

    Ok(dbg!(res.city))
}

// Compute the Australian Apparent Temperature (AT),
// using the metric formula found on Wikipedia.
// If using imperial units, we must first convert to metric.
fn australian_apparent_temp(
    raw_temp: f64,
    raw_humidity: f64,
    raw_wind_speed: f64,
    units: OpenWeatherMapUnits,
) -> f64 {
    let temp_celsius = match units {
        OpenWeatherMapUnits::Metric => raw_temp,
        OpenWeatherMapUnits::Imperial => (raw_temp - 32.0) * 0.556,
    };

    let exponent = 17.27 * temp_celsius / (237.7 + temp_celsius);
    let water_vapor_pressure = raw_humidity * 0.06105 * exponent.exp();

    let metric_wind_speed = match units {
        OpenWeatherMapUnits::Metric => raw_wind_speed,
        OpenWeatherMapUnits::Imperial => raw_wind_speed * 0.447,
    };

    let metric_apparent_temp =
        temp_celsius + 0.33 * water_vapor_pressure - 0.7 * metric_wind_speed - 4.0;

    match units {
        OpenWeatherMapUnits::Metric => metric_apparent_temp,
        OpenWeatherMapUnits::Imperial => 1.8 * metric_apparent_temp + 32.,
    }
}

// Convert wind direction in azimuth degrees to abbreviation names
fn convert_wind_direction(direction_opt: Option<f64>) -> String {
    match direction_opt {
        Some(direction) => match direction.round() as i64 {
            24..=68 => "NE".to_string(),
            69..=113 => "E".to_string(),
            114..=158 => "SE".to_string(),
            159..=203 => "S".to_string(),
            204..=248 => "SW".to_string(),
            249..=293 => "W".to_string(),
            294..=338 => "NW".to_string(),
            _ => "N".to_string(),
        },
        None => "-".to_string(),
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
struct WeatherConfig {
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
    format: FormatTemplate,
    service: WeatherService,
    autolocate: bool,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(600),
            format: Default::default(),
            service: WeatherService::default(),
            autolocate: false,
        }
    }
}

pub fn spawn(block_config: toml::Value, mut api: CommonApi, _: EventsRxGetter) -> BlockHandle {
    tokio::spawn(async move {
        let block_config = WeatherConfig::deserialize(block_config).config_error()?;
        let format = block_config
            .format
            .clone()
            .or_default("{weather} {temp}\u{00b0}")?;

        loop {
            let data = block_config.service.get(block_config.autolocate).await?;

            let apparent_temp = australian_apparent_temp(
                data.main.temp,
                data.main.humidity,
                data.wind.speed,
                block_config.service.units(),
            );

            let kmh_wind_speed = (3600. / 1000.)
                * match block_config.service.units() {
                    OpenWeatherMapUnits::Metric => data.wind.speed,
                    OpenWeatherMapUnits::Imperial => 0.447 * data.wind.speed,
                };

            let keys = map! {
                "weather" => Value::from_string(data.weather[0].main.to_string()),
                "temp" => Value::from_float(data.main.temp),
                "humidity" => Value::from_float(data.main.humidity),
                "apparent" => Value::from_float(apparent_temp),
                "wind" => Value::from_float(kmh_wind_speed),
                "wind_kmh" => Value::from_float(kmh_wind_speed),
                "direction" => Value::from_string(convert_wind_direction(data.wind.deg)),
                "location" => Value::from_string(data.name),
            };

            let icon = match data.weather[0].main.as_str() {
                "Clear" => "weather_sun",
                "Rain" | "Drizzle" => "weather_rain",
                "Clouds" | "Fog" | "Mist" => "weather_clouds",
                "Thunderstorm" => "weather_thunder",
                "Snow" => "weather_snow",
                _ => "weather_default",
            };

            let widget = api
                .new_widget()
                .with_text(format.render(&keys)?)
                .with_icon(icon)?
                .get_data();
            api.send_widgets(vec![widget]).await?;

            tokio::time::sleep(block_config.interval).await;
        }
    })
}
