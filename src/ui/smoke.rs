//! The smoke behind the music shown full screen: noise warped by noise, flowing by a time of its own (faster as the
//! music swells, never backwards), in the cover's two colours, under a fine screen of dots and darker towards the
//! edges. A pixel shader draws it on Direct3D 11 into a texture of its own, smaller than the screen (the smoke is
//! soft: half as many pixels each way look the same), which Direct2D then draws stretched over the frame.

use std::mem::size_of;

use windows::core::{s, Interface, Result};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_INTERPOLATION_MODE_LINEAR,
};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader, ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGISurface;

use super::gfx::{Frame, Gfx};

/// How many of the screen's pixels the smoke is drawn with, each way.
pub const SCALE: f32 = 0.5;

const SHADER: &str = r#"
cbuffer Smoke : register(b0) {
    float flow;     // its own time: how far it has flowed
    float swell;    // the music's slow-following swell (0 to 1)
    float aspect;   // width over height
    float dots;     // the screen of dots' pitch, in the texture's pixels
    float4 tint;    // the cover's colour
    float4 tint2;   // its second
};

float hash(float2 p) { return frac(sin(dot(p, float2(127.1, 311.7))) * 43758.5453); }
float noise(float2 p) {
    float2 i = floor(p), f = frac(p), u = f * f * (3.0 - 2.0 * f);
    return lerp(lerp(hash(i), hash(i + float2(1, 0)), u.x), lerp(hash(i + float2(0, 1)), hash(i + float2(1, 1)), u.x), u.y);
}
float fbm(float2 p) {
    float v = 0.0, a = 0.5;
    [unroll] for (int k = 0; k < 6; k++) { v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; }
    return v;
}

struct Out { float4 position : SV_Position; float2 uv : TEXCOORD0; };

// One triangle covering the target; uv from the bottom left, as the smoke was drawn.
Out vs(uint id : SV_VertexID) {
    float2 t = float2((id << 1) & 2, id & 2);
    Out o;
    o.position = float4(t * float2(2, -2) + float2(-1, 1), 0, 1);
    o.uv = float2(t.x, 1.0 - t.y);
    return o;
}

float4 ps(Out i) : SV_Target {
    float2 p = float2(i.uv.x * aspect, i.uv.y);
    float2 q = float2(fbm(p * 1.6 + flow * 0.05), fbm(p * 1.6 + float2(5.2, 1.3) - flow * 0.04));
    float2 r = float2(fbm(p * 1.8 + 3.0 * q + float2(1.7, 9.2) + flow * 0.09), fbm(p * 1.8 + 3.0 * q + float2(8.3, 2.8) + flow * 0.07));
    float f = fbm(p * 1.4 + 2.5 * r);
    float3 c = lerp(tint.rgb * 0.1, tint.rgb * 0.5, smoothstep(0.25, 0.85, f));
    c = lerp(c, tint2.rgb * 0.95, smoothstep(0.62, 1.0, f * f * 1.6) * 0.75);
    c *= 0.8 + 0.35 * swell;
    float2 cell = frac(i.position.xy / dots) - 0.5;
    c += 0.035 * smoothstep(0.25, 0.0, length(cell));
    c *= 1.0 - 0.45 * length(i.uv - 0.5);
    return float4(c, 1.0);
}
"#;

/// What the shader is given, as its constant buffer lays it out.
#[repr(C)]
struct Constants {
    flow: f32,
    swell: f32,
    aspect: f32,
    dots: f32,
    tint: [f32; 4],
    tint2: [f32; 4],
}

/// How the smoke stands at a frame: how far it has flowed, the music's swell, and the cover's two colours (0 to 1).
pub struct Now {
    pub flow: f32,
    pub swell: f32,
    pub tint: [f32; 3],
    pub tint2: [f32; 3],
}

pub struct Smoke {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    constants: ID3D11Buffer,
    /// Direct2D's side of the device, which makes the texture a bitmap.
    d2d: ID2D1DeviceContext,
    /// The texture drawn into, its size, its view as a target, and the same as Direct2D's bitmap.
    target: Option<((u32, u32), ID3D11RenderTargetView, ID2D1Bitmap1)>,
}

impl Smoke {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let device = gfx.d3d()?;
        let context = unsafe { device.GetImmediateContext()? };
        let compiled = |entry: windows::core::PCSTR, target: windows::core::PCSTR| -> Result<Vec<u8>> {
            let mut code: Option<ID3DBlob> = None;
            unsafe { D3DCompile(SHADER.as_ptr().cast(), SHADER.len(), s!("smoke"), None, None, entry, target, 0, 0, &mut code, None)? };
            let code = code.ok_or_else(windows::core::Error::empty)?;
            Ok(unsafe { std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize()) }.to_vec())
        };
        let (mut vertex, mut pixel, mut constants) = (None, None, None);
        unsafe {
            device.CreateVertexShader(&compiled(s!("vs"), s!("vs_4_0"))?, None, Some(&mut vertex))?;
            device.CreatePixelShader(&compiled(s!("ps"), s!("ps_4_0"))?, None, Some(&mut pixel))?;
            let desc = D3D11_BUFFER_DESC { ByteWidth: size_of::<Constants>() as u32, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, ..Default::default() };
            device.CreateBuffer(&desc, None, Some(&mut constants))?;
        }
        let d2d = unsafe { gfx.device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        let missing = windows::core::Error::empty;
        Ok(Smoke { device, context, vertex: vertex.ok_or_else(missing)?, pixel: pixel.ok_or_else(missing)?, constants: constants.ok_or_else(missing)?, d2d, target: None })
    }

    /// Draws the smoke as it stands `now` into its texture, `pixels` large (made anew when the size changes), for
    /// `paint` to draw. Done before the frame's drawing begins, so Direct3D's work does not come between Direct2D's.
    pub fn render(&mut self, pixels: (u32, u32), now: &Now) -> Result<()> {
        let pixels = (pixels.0.max(1), pixels.1.max(1));
        if self.target.as_ref().map(|(size, ..)| *size) != Some(pixels) {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: pixels.0,
                Height: pixels.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                ..Default::default()
            };
            let (mut texture, mut view): (Option<ID3D11Texture2D>, Option<ID3D11RenderTargetView>) = (None, None);
            unsafe {
                self.device.CreateTexture2D(&desc, None, Some(&mut texture))?;
                let texture = texture.ok_or_else(windows::core::Error::empty)?;
                self.device.CreateRenderTargetView(&texture, None, Some(&mut view))?;
                let properties = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                    ..Default::default()
                };
                let bitmap = self.d2d.CreateBitmapFromDxgiSurface(&texture.cast::<IDXGISurface>()?, Some(&properties))?;
                self.target = Some((pixels, view.ok_or_else(windows::core::Error::empty)?, bitmap));
            }
        }
        let (size, view, _) = self.target.as_ref().unwrap();
        let constants = Constants {
            flow: now.flow,
            swell: now.swell,
            aspect: size.0 as f32 / size.1 as f32,
            dots: 4.0 * SCALE,
            tint: [now.tint[0], now.tint[1], now.tint[2], 1.0],
            tint2: [now.tint2[0], now.tint2[1], now.tint2[2], 1.0],
        };
        unsafe {
            self.context.UpdateSubresource(&self.constants, 0, None, (&constants as *const Constants).cast(), 0, 0);
            self.context.OMSetRenderTargets(Some(&[Some(view.clone())]), None);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT { Width: size.0 as f32, Height: size.1 as f32, MaxDepth: 1.0, ..Default::default() }]));
            self.context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.IASetInputLayout(None);
            self.context.VSSetShader(&self.vertex, None);
            self.context.PSSetShader(&self.pixel, None);
            self.context.PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            self.context.Draw(3, 0);
            self.context.OMSetRenderTargets(None, None);
        }
        Ok(())
    }

    /// Draws what `render` drew over `rect` (DIPs) of `frame`, stretched smoothly.
    pub fn paint(&self, frame: &Frame, rect: D2D_RECT_F) {
        if let Some((_, _, bitmap)) = &self.target {
            unsafe { frame.dc.DrawBitmap(bitmap, Some(&rect), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
        }
    }
}

#[cfg(test)]
mod tests {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING};

    /// The smoke drawn once, read back and written as raw BGRA (width, height, then the rows) to the file named by
    /// GLANCE_SMOKE_OUT, to be looked at.
    #[test]
    #[ignore = "draws on this machine's graphics card"]
    fn draws_the_smoke() {
        let gfx = super::Gfx::new().unwrap();
        let mut smoke = super::Smoke::new(&gfx).unwrap();
        let size = (960u32, 540u32);
        smoke.render(size, &super::Now { flow: 12.0, swell: 0.6, tint: [0.92, 0.62, 0.55], tint2: [0.98, 0.85, 0.80] }).unwrap();
        let (_, view, _) = smoke.target.as_ref().unwrap();
        let texture: ID3D11Texture2D = unsafe { view.GetResource() }.unwrap().cast().unwrap();
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        unsafe { smoke.device.CreateTexture2D(&desc, None, Some(&mut staging)) }.unwrap();
        let staging = staging.unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&size.0.to_le_bytes());
        out.extend_from_slice(&size.1.to_le_bytes());
        unsafe {
            smoke.context.CopyResource(&staging, &texture);
            let mut mapped = Default::default();
            smoke.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
            for y in 0..size.1 as usize {
                out.extend_from_slice(std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize), size.0 as usize * 4));
            }
            smoke.context.Unmap(&staging, 0);
        }
        std::fs::write(std::env::var("GLANCE_SMOKE_OUT").unwrap(), out).unwrap();
    }
}
