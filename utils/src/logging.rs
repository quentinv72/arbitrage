use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::Config;
use log4rs::encode::json::JsonEncoder;
use log::LevelFilter;

pub fn setup_logging(is_production: bool, app_name: &str) {
    let dev_stdout = ConsoleAppender::builder().build();
    let prod_stdout = ConsoleAppender::builder()
        .encoder(Box::new(JsonEncoder::new()))
        .build();
    if !is_production {
        let config = Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(dev_stdout)))
            .logger(Logger::builder().build(app_name, LevelFilter::Debug))
            .logger(Logger::builder().build("pools_graph", LevelFilter::Debug))
            .build(Root::builder().appender("stdout").build(LevelFilter::Warn))
            .unwrap();
        log4rs::init_config(config).expect("Should set up logger in debug mode for app");
    } else {
        let config = Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(prod_stdout)))
            .logger(Logger::builder().build(app_name, LevelFilter::Info))
            .logger(Logger::builder().build("pools_graph", LevelFilter::Info))
            .build(Root::builder().appender("stdout").build(LevelFilter::Warn))
            .unwrap();
        log4rs::init_config(config).expect("Should set up logger in debug mode for app");
    }
}
