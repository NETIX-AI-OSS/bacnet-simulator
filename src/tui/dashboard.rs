use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::snapshot::AppSnapshot;

pub fn draw(f: &mut Frame, area: ratatui::layout::Rect, snap: &AppSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Min(3),
        ])
        .split(area);

    let listen = if snap.listening {
        "listening"
    } else {
        "not listening"
    };
    let last_client = snap
        .last_client
        .as_deref()
        .unwrap_or("—");

    let summary = vec![
        Line::from(vec![
            Span::styled("Building: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&snap.building_name),
        ]),
        Line::from(vec![
            Span::styled("Config: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&snap.config_path),
        ]),
        Line::from(vec![
            Span::styled("BACnet: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("0.0.0.0:{} ({listen})", snap.port)),
        ]),
        Line::from(vec![
            Span::styled("Uptime: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}s", snap.uptime_secs)),
        ]),
        Line::from(vec![
            Span::styled("Devices / Points: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} / {}", snap.device_count, snap.point_count)),
        ]),
        Line::from(vec![
            Span::styled("Occupancy: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:.1}%", snap.occupancy_pct)),
            Span::styled("   Outside: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:.1} °C", snap.outside_temp_c)),
        ]),
        Line::from(vec![
            Span::styled("Requests: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(
                "Who-Is {} | ReadProperty {} | RPM {}",
                snap.who_is, snap.read_property, snap.read_property_multiple
            )),
        ]),
        Line::from(vec![
            Span::styled("Last client: ", Style::default().fg(Color::DarkGray)),
            Span::raw(last_client),
        ]),
    ];

    f.render_widget(
        Paragraph::new(summary).block(Block::default().borders(Borders::NONE)),
        chunks[0],
    );

    let log_text: Vec<Line> = snap
        .log_lines
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|l| Line::from(l.as_str()))
        .collect();
    f.render_widget(
        Paragraph::new(log_text).block(Block::default().title(" Log ").borders(Borders::ALL)),
        chunks[1],
    );
}
