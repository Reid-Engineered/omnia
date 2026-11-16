use super::*;
use gpui::{AppContext, Context, IntoElement, Render, WindowBounds, WindowOptions};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
struct Probe {
    scene: Rc<RefCell<dyn BenchScene>>,
}
impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::scene::SceneElement {
            scene: self.scene.clone(),
            camera: Camera::default(),
            paint_time: Rc::new(Cell::new(None)),
        }
    }
}
struct Bare;
impl BenchScene for Bare {
    fn paint(&mut self, b: Bounds<Pixels>, _: Camera, w: &mut Window, _: &mut App) {
        not_ours(|| w.paint_quad(fill(b, rgb(PAPER))));
    }
}
struct Quality {
    symbols: Vec<(SharedString, Vec<u8>)>,
}
impl BenchScene for Quality {
    fn paint(&mut self, _: Bounds<Pixels>, _: Camera, window: &mut Window, cx: &mut App) {
        not_ours(|| {
            window.paint_quad(fill(
                Bounds::new(point(px(0.), px(0.)), size(px(1024.), px(960.))),
                rgb(0xffffff),
            ))
        });
        let scale = not_ours(|| window.scale_factor());
        for (i, (key, data)) in self.symbols.iter().enumerate() {
            for o in 0..8 {
                let b = Bounds::new(
                    point(
                        px((i / 15 * 8 + o) as f32 * 64. + 8.),
                        px((i % 15) as f32 * 64. + 8.),
                    ),
                    size(px(48.), px(48.)),
                );
                not_ours(|| {
                    window.paint_svg(
                        b,
                        key.clone(),
                        Some(data),
                        orientation(b, scale, o),
                        rgb(0).into(),
                        cx,
                    )
                })
                .expect("quality SVG");
                not_ours(|| {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                px((i / 15 * 8 + o) as f32 * 64. + 4.),
                                px((i % 15) as f32 * 64. + 58.),
                            ),
                            size(px(56.), px(1. / scale)),
                        ),
                        rgb(0),
                    ))
                });
            }
        }
    }
}
pub fn run(config: Config, requested: (f32, f32)) -> Result<Value, String> {
    harness::watchdog("readback", Duration::from_secs(90));
    let slot = Rc::new(RefCell::new(None));
    let result = slot.clone();
    let start = Instant::now();
    gpui_platform::application().run(move |cx:&mut App| {
        let scene:Rc<RefCell<dyn BenchScene>>=if config.scene==3 {
            Rc::new(RefCell::new(Quality{symbols:Scene::new(config).symbols}))
        }else{Rc::new(RefCell::new(Bare))};
        let dimensions=if config.scene==3 {(1024.,960.)}else{requested};
        let b=Bounds::new(point(px(0.),px(0.)),size(px(dimensions.0),px(dimensions.1)));
        let opened=cx.open_window(WindowOptions {show:false,focus:false,window_bounds:Some(WindowBounds::Windowed(b)),window_min_size:Some(b.size),..Default::default()},
            move |window,cx|{window.resize(b.size);cx.new(|_|Probe{scene})});
        let Ok(win)=opened else {*slot.borrow_mut()=Some(Err("hidden window failed".into()));cx.quit();return;};
        cx.spawn(async move |cx| {
            let report=cx.update(|cx|cx.update_window(win.into(),|_,window,cx|->Result<Value,String>{
                window.draw(cx).clear(cx);
                let first_draw_ms=start.elapsed().as_secs_f64()*1000.;
                let image=window.render_to_image().map_err(|e|e.to_string())?;
                let expected=((dimensions.0*window.scale_factor()).round() as u32,(dimensions.1*window.scale_factor()).round() as u32);
                if (image.width(),image.height())!=expected {return Err(format!("readback clipped: got {}x{}, expected {}x{}",image.width(),image.height(),expected.0,expected.1));}
                let path=std::path::PathBuf::from(format!("target/spike-s{}.png",config.scene));
                image.save(&path).map_err(|e|e.to_string())?;
                let mut report=json!({"schema":crate::compare::SCHEMA,"config":config.json(),"kind":"spike-readback","spike":config.json(),
                    "environment":harness::environment(window,cx),"image":path,
                    "metrics":{"first_draw_ms":first_draw_ms,"private_mb":crate::sys::memory().map(|m|m.private_bytes as f64/1048576.),"image_width":image.width(),"image_height":image.height()},
                    "limitations":["no-GPU readback requires a separate host; live monitor changes are recorded by s8 --mode monitors"]});
                if config.scene==3 {
                    let scale=window.scale_factor();
                    let mut reference=resvg::tiny_skia::Pixmap::new(image.width(),image.height()).ok_or("reference size")?;
                    reference.fill(resvg::tiny_skia::Color::WHITE);
                    for (i,(_,data)) in Scene::new(config).symbols.iter().enumerate() {for o in 0..8 {
                        let tree=resvg::usvg::Tree::from_data(data,&Default::default()).map_err(|e|e.to_string())?;
                        let b=Bounds::new(point(px((i/15*8+o) as f32*64.+8.),px((i%15) as f32*64.+8.)),size(px(48.),px(48.)));
                        let m=orientation(b,scale,o);
                        let a=m.rotation_scale;
                        let base=resvg::tiny_skia::Transform::from_row(a[0][0],a[1][0],a[0][1],a[1][1],m.translation[0],m.translation[1]);
                        let t=base.pre_translate(f32::from(b.origin.x)*scale,f32::from(b.origin.y)*scale)
                            .pre_scale(48.*scale/tree.size().width(),48.*scale/tree.size().width());
                        resvg::render(&tree,t,&mut reference.as_mut());
                    }}
                    reference.save_png("target/spike-s3-reference.png").map_err(|e|e.to_string())?;
                    let mut changed=0;let mut ink=0;let mut sum=0_u64;
                    let mut by_orientation=[[0_u64;4];8];
                    // Exclude the quad wire comparison strips beneath each symbol.
                    for y in 0..image.height() {if (y as f32/scale)%64.>=56. {continue;}
                        for x in 0..image.width() {
                            let actual=image.get_pixel(x,y).0;
                            let expected=reference.pixel(x,y).ok_or("reference pixel")?;
                            let d=actual[0].abs_diff(expected.red());
                            let o=((x as f32/scale/64.) as usize)%8;
                            if actual[0]<250 || expected.red()<250 {by_orientation[o][0]+=1;}
                            if d>2 {by_orientation[o][1]+=1;}
                            if actual[0]>2 && actual[0]<253 {by_orientation[o][2]+=1;}
                            if expected.red()>2 && expected.red()<253 {by_orientation[o][3]+=1;}
                            if actual[0]<250 || expected.red()<250 {ink+=1;}
                            if d>2 {changed+=1;} sum+=u64::from(d);
                        }
                    }
                    if ink==0 {return Err("blank SVG probe".into());}
                    report["orientation_columns"]=json!(["ink_union","changed_gt_2","gpui_antialiased","reference_antialiased"]);
                    report["by_orientation"]=json!(by_orientation);
                    report["metrics"]["changed_pixels_gt_2"]=json!(changed);
                    report["metrics"]["ink_pixels"]=json!(ink);
                    report["metrics"]["changed_fraction_of_ink"]=json!(changed as f64/ink as f64);
                    report["metrics"]["absolute_channel_error"]=json!(sum);
                }
                Ok(report)
            }).map_err(|e|e.to_string()).and_then(|r|r));
            *slot.borrow_mut()=Some(report);cx.update(|cx|cx.quit());
        }).detach();
    });
    result
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err("readback exited without result".into()))
}

/// Probe each attached display with its own real hidden window. This records live DPI,
/// not a test-support scale override.
pub fn displays() -> Result<Value, String> {
    harness::watchdog("display readbacks", Duration::from_secs(60));
    let result = Rc::new(RefCell::new(None));
    let slot = result.clone();
    gpui_platform::application().run(move |cx:&mut App| {
        let mut windows=Vec::new();
        for (index,display) in cx.displays().into_iter().enumerate() {
            let b=Bounds::centered(Some(display.id()),size(px(512.),px(320.)),cx);
            let opened=cx.open_window(WindowOptions {
                show:false,focus:false,display_id:Some(display.id()),
                window_bounds:Some(WindowBounds::Windowed(b)),window_min_size:Some(b.size),
                ..Default::default()
            },|window,cx|{
                window.resize(b.size);
                cx.new(|_|Probe{scene:Rc::new(RefCell::new(Bare))})
            });
            match opened {
                Ok(win)=>windows.push((index,format!("{:?}",display.bounds()),win)),
                Err(e)=>{*slot.borrow_mut()=Some(Err(e.to_string()));cx.quit();return;}
            }
        }
        cx.spawn(async move |cx|{
            let report=cx.update(|cx|->Result<Value,String>{
                let mut records=Vec::new();
                for (index,bounds,win) in windows {
                    records.push(cx.update_window(win.into(),|_,window,cx|->Result<Value,String>{
                        window.draw(cx).clear(cx);
                        let image=window.render_to_image().map_err(|e|e.to_string())?;
                        let path=format!("target/spike-display-{index}.png");
                        image.save(&path).map_err(|e|e.to_string())?;
                        Ok(json!({"bounds_logical":bounds,"environment":harness::environment(window,cx),
                            "image":path,"image_pixels":[image.width(),image.height()],
                            "center_rgba":image.get_pixel(image.width()/2,image.height()/2).0}))
                    }).map_err(|e|e.to_string())??);
                }
                Ok(json!({"schema":crate::compare::SCHEMA,"kind":"spike-displays","displays":records}))
            });
            *slot.borrow_mut()=Some(report);
            cx.update(|cx|cx.quit());
        }).detach();
    });
    result
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err("display probes exited without a result".into()))
}
