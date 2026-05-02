// SPDX-License-Identifier: GPL-2.0+
/*
 * evo_raw - raw USB control transfer access for Audient EVO 8
 *
 * This module binds to the Audient EVO 8 USB device and exposes a misc
 * device (/dev/evo8). A single ioctl (EVO_CTRL_TRANSFER) lets userspace
 * send/receive arbitrary USB control transfers via the kernel's
 * usb_control_msg(), bypassing usbfs interface-ownership checks.
 * snd-usb-audio continues to handle audio streaming undisturbed.
 *
 * AUTHORS:
 *   Derived from vanzaho/audient-evo-py (public domain)
 *   https://github.com/vanzaho/audient-evo-py
 *   Adapted for evo-control (EVO 8 only): Brian Nadon, 2026
 */

#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/usb.h>

#define AUDIENT_VID 0x2708
#define EVO8_PID    0x0007

#define EVO_MAX_DATA 256

/* ioctl payload - matches the struct userspace packs */
struct evo_ctrl_xfer {
	__u8  bRequestType;
	__u8  bRequest;
	__u16 wValue;
	__u16 wIndex;
	__u16 wLength;
	__u8  data[EVO_MAX_DATA];
};

/* ioctl number: type='E' (0x45), nr=0, read+write, size of struct */
#define EVO_CTRL_TRANSFER _IOWR('E', 0, struct evo_ctrl_xfer)

struct evo_device {
	struct usb_device *udev;
	struct miscdevice  misc;
	struct mutex       lock;
};

static int evo_open(struct inode *inode, struct file *file)
{
	struct evo_device *dev =
		container_of(file->private_data, struct evo_device, misc);
	file->private_data = dev;
	return 0;
}

static long evo_ioctl(struct file *file, unsigned int cmd, unsigned long arg)
{
	struct evo_device  *dev = file->private_data;
	struct evo_ctrl_xfer xfer;
	unsigned int pipe;
	void        *dmabuf;
	int          ret;

	if (cmd != EVO_CTRL_TRANSFER)
		return -ENOTTY;

	if (copy_from_user(&xfer, (void __user *)arg, sizeof(xfer)))
		return -EFAULT;

	if (xfer.wLength > EVO_MAX_DATA)
		return -EINVAL;

	/* usb_control_msg requires a DMA-able buffer, not stack memory */
	dmabuf = kmalloc(xfer.wLength, GFP_KERNEL);
	if (!dmabuf)
		return -ENOMEM;

	if (!(xfer.bRequestType & USB_DIR_IN))
		memcpy(dmabuf, xfer.data, xfer.wLength);

	mutex_lock(&dev->lock);

	if (!dev->udev) {
		mutex_unlock(&dev->lock);
		kfree(dmabuf);
		return -ENODEV;
	}

	if (xfer.bRequestType & USB_DIR_IN)
		pipe = usb_rcvctrlpipe(dev->udev, 0);
	else
		pipe = usb_sndctrlpipe(dev->udev, 0);

	ret = usb_control_msg(dev->udev, pipe, xfer.bRequest,
			      xfer.bRequestType, xfer.wValue, xfer.wIndex,
			      dmabuf, xfer.wLength, 1000 /* ms */);

	mutex_unlock(&dev->lock);

	if (ret < 0) {
		kfree(dmabuf);
		return ret;
	}

	if (xfer.bRequestType & USB_DIR_IN) {
		memcpy(xfer.data, dmabuf, ret);
		xfer.wLength = ret;
		if (copy_to_user((void __user *)arg, &xfer, sizeof(xfer))) {
			kfree(dmabuf);
			return -EFAULT;
		}
	}

	kfree(dmabuf);
	return ret;
}

static const struct file_operations evo_fops = {
	.owner          = THIS_MODULE,
	.open           = evo_open,
	.unlocked_ioctl = evo_ioctl,
};

static int evo_probe(struct usb_interface *intf,
		     const struct usb_device_id *id)
{
	struct usb_device *udev = interface_to_usbdev(intf);
	struct evo_device *dev;

	/*
	 * snd-usb-audio claims interfaces 0-2 (audio control + streaming).
	 * Interface 3 (DFU) is left unbound — we grab it only to obtain the
	 * usb_device handle. All control transfers go through endpoint 0.
	 */
	if (intf->cur_altsetting->desc.bInterfaceNumber != 3)
		return -ENODEV;

	dev = kzalloc(sizeof(*dev), GFP_KERNEL);
	if (!dev)
		return -ENOMEM;

	mutex_init(&dev->lock);
	dev->udev      = usb_get_dev(udev);
	dev->misc.minor = MISC_DYNAMIC_MINOR;
	dev->misc.name  = "evo8";
	dev->misc.fops  = &evo_fops;

	if (misc_register(&dev->misc)) {
		dev_err(&intf->dev, "failed to register /dev/evo8\n");
		usb_put_dev(dev->udev);
		kfree(dev);
		return -ENODEV;
	}

	dev_info(&intf->dev, "Audient EVO 8 raw control registered at /dev/evo8\n");
	usb_set_intfdata(intf, dev);
	return 0;
}

static void evo_disconnect(struct usb_interface *intf)
{
	struct evo_device *dev = usb_get_intfdata(intf);

	if (!dev)
		return;

	mutex_lock(&dev->lock);
	misc_deregister(&dev->misc);
	usb_put_dev(dev->udev);
	dev->udev = NULL;
	mutex_unlock(&dev->lock);

	dev_info(&intf->dev, "Audient EVO 8 raw control disconnected\n");
	kfree(dev);
}

static const struct usb_device_id evo_id_table[] = {
	{ USB_DEVICE(AUDIENT_VID, EVO8_PID) },
	{}
};
MODULE_DEVICE_TABLE(usb, evo_id_table);

static struct usb_driver evo_driver = {
	.name      = "evo_raw",
	.id_table  = evo_id_table,
	.probe     = evo_probe,
	.disconnect = evo_disconnect,
};
module_usb_driver(evo_driver);

MODULE_LICENSE("GPL v2");
MODULE_AUTHOR("vanzaho/audient-evo-py contributors; Brian Nadon");
MODULE_DESCRIPTION("Raw USB control transfer access for Audient EVO 8");
