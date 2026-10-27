package cafe.token.android;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Rect;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewConfiguration;
import java.time.LocalTime;

final class CafeView extends View {
    private static final int WIDTH = 480, HEIGHT = 272;
    private final int[] pixels = new int[WIDTH * HEIGHT];
    private final Bitmap bitmap = Bitmap.createBitmap(WIDTH, HEIGHT, Bitmap.Config.ARGB_8888);
    private final Paint paint = new Paint();
    private final Rect destination = new Rect();
    private boolean running, pressed, dragged;
    private float downX, downY;
    private final int slop;
    private final Runnable animation = new Runnable() {
        @Override public void run() {
            if (!running) return;
            LocalTime now = LocalTime.now();
            NativeCafe.render(pixels, now.getHour(), now.getMinute(), now.getSecond());
            bitmap.setPixels(pixels, 0, WIDTH, 0, 0, WIDTH, HEIGHT);
            invalidate();
            postDelayed(this, 33);
        }
    };

    CafeView(Context context) {
        super(context);
        slop = ViewConfiguration.get(context).getScaledTouchSlop();
        paint.setFilterBitmap(false);
        setContentDescription(context.getString(R.string.cafe_description));
        setFocusable(true);
        setClickable(true);
    }

    void start() { if (!running) { running = true; post(animation); } }
    void stop() { running = false; removeCallbacks(animation); }

    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        float scale = Math.min(getWidth() / (float)WIDTH, getHeight() / (float)HEIGHT);
        if (scale >= 1) scale = (float)Math.floor(scale);
        int width = Math.round(WIDTH * scale), height = Math.round(HEIGHT * scale);
        int left = (getWidth() - width) / 2, top = (getHeight() - height) / 2;
        destination.set(left, top, left + width, top + height);
        canvas.drawBitmap(bitmap, null, destination, paint);
    }

    private void dispatch(float x, float y, boolean scroll) {
        if (!destination.contains((int)x, (int)y) || destination.width() == 0) return;
        NativeCafe.touch((int)((x - destination.left) * WIDTH / destination.width()),
            (int)((y - destination.top) * HEIGHT / destination.height()), scroll);
    }

    @Override public boolean onTouchEvent(MotionEvent event) {
        float x = event.getX(), y = event.getY();
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                pressed = destination.contains((int)x, (int)y);
                dragged = false; downX = x; downY = y;
                return pressed;
            case MotionEvent.ACTION_MOVE:
                if (pressed && Math.hypot(x - downX, y - downY) > slop) dragged = true;
                // Only gestures starting in the scrollbar may scroll into another region.
                if (pressed && dragged && destination.width() > 0) {
                    int startX = (int)((downX - destination.left) * WIDTH / destination.width());
                    if (startX >= 436) dispatch(x, y, true);
                }
                return pressed;
            case MotionEvent.ACTION_UP:
                if (pressed && !dragged) { dispatch(x, y, false); performClick(); }
                pressed = false; return true;
            case MotionEvent.ACTION_POINTER_DOWN:
            case MotionEvent.ACTION_CANCEL:
                pressed = false; return true;
            default: return pressed;
        }
    }

    @Override public boolean performClick() { super.performClick(); return true; }
}
