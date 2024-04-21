use log4rs::append::console::ConsoleAppender;
use log4rs::Config;
use log4rs::config::{Appender, Logger, Root};
use log::LevelFilter;

use crate::env::Env;

pub fn setup_logging(env: Env, app_name: &str) {
    let stdout = ConsoleAppender::builder().build();
    if env.is_staging() {
        let config = Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(Logger::builder().build(app_name, LevelFilter::Debug))
            .logger(Logger::builder().build("pools_graph", LevelFilter::Info))
            .build(Root::builder().appender("stdout").build(LevelFilter::Warn))
            .unwrap();
        log4rs::init_config(config).expect("Should set up logger in debug mode for app");
    } else {
        let config = Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(Logger::builder().build(app_name, LevelFilter::Info))
            .logger(Logger::builder().build("pools_graph", LevelFilter::Info))
            .build(Root::builder().appender("stdout").build(LevelFilter::Warn))
            .unwrap();
        log4rs::init_config(config).expect("Should set up logger in debug mode for app");
    }
}
