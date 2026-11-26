package dev.gpui.mobile;

import android.content.Context;
import android.util.Log;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;

/**
 * SurfaceView whose {@link Surface} is handed to libmpv on the Rust side.
 *
 * <p>The view is created by {@code GpuiPlatformView.createView} for the
 * {@code "mpv_surface"} platform-view type (crates/streamtube-android). When
 * Android reports the surface ready/destroyed, the static native callbacks
 * below notify the Rust player, which creates a JNI global reference to the
 * {@code Surface} object and passes its pointer to libmpv as the {@code wid}
 * option (the pattern proven by upstream mpv-android).</p>
 *
 * <p>{@code setZOrderOnTop(true)} is required: NativeActivity renders GPUI on
 * its own SurfaceView, and a media-overlay surface would stay behind it.</p>
 */
public class StreamTubeSurfaceView extends SurfaceView implements SurfaceHolder.Callback {

    private static final String TAG = "StreamTubeSurfaceView";

    /** The platform-view id Rust passed as the "view_id" creation param. */
    private final long viewId;

    public StreamTubeSurfaceView(Context context, long viewId) {
        super(context);
        this.viewId = viewId;
        // Must be called before the view is attached to the window.
        setZOrderOnTop(true);
        getHolder().addCallback(this);
        Log.i(TAG, "created for view_id=" + viewId);
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        Log.i(TAG, "surfaceCreated id=" + viewId + " valid=" + holder.getSurface().isValid());
        nativeSurfaceCreated(viewId, holder.getSurface());
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        Log.i(TAG, "surfaceChanged id=" + viewId + " " + width + "x" + height);
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        Log.i(TAG, "surfaceDestroyed id=" + viewId);
        nativeSurfaceDestroyed(viewId);
    }

    // Implemented in crates/streamtube-android/src/player.rs.
    private static native void nativeSurfaceCreated(long viewId, Surface surface);
    private static native void nativeSurfaceDestroyed(long viewId);
}
